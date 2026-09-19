//! pet-agent 原生 Win32 输入窗口 + 全局快捷键 + 托盘 + 常驻消息循环。
//!
//! 线程边界：
//! - UI 线程：窗口 / 控件 / 消息循环 / 托盘 / 快捷键；只与 worker 通过 channel 交互。
//! - AI worker 线程：HTTP 请求（见 worker.rs）。
//! - LLM 请求绝不阻塞 UI 线程。

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, GET_STOCK_OBJECT_FLAGS, HBRUSH};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_SPACE,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::AgentConfig;
use crate::ipc;
use crate::mapper;
use crate::worker::{AiRequest, AiWorker};
use crate::{autostart, log_line, single_instance};

const ID_EDIT: i32 = 1001;
const ID_SEND: i32 = 1002;
const ID_STATUS: i32 = 1003;
const HOTKEY_ID: i32 = 1;
const TRAY_CALLBACK: u32 = WM_APP + 10;

// 托盘菜单命令
const CMD_OPEN: usize = 1;
const CMD_CLEAR: usize = 2;
const CMD_AUTOSTART: usize = 3;
const CMD_EXIT: usize = 4;

thread_local! {
    static CTX: RefCell<Option<UiContext>> = RefCell::new(None);
}

struct UiContext {
    worker: AiWorker,
    #[allow(dead_code)]
    cfg: AgentConfig,
    generation: u64,
    active_gen: Option<u64>,
    thinking: bool,
    wake_msg: u32,
    tray_added: bool,
    last_reply: String,
    provider_name: String,
    /// 短期多轮上下文（与 chat 模式同一套 Conversation）。
    conv: crate::context::Conversation,
    /// 待写入 history 的 (generation, user_text)。
    pending_user: Option<(u64, String)>,
    /// TTS 控制器。
    tts: crate::tts::TtsController,
}

pub fn run_ui(provider_override: Option<&str>) {
    // panic hook：把 panic 写入日志，避免消息循环静默崩溃
    std::panic::set_hook(Box::new(|info| {
        crate::log_line(&format!("PANIC: {info}"));
    }));
    let cfg = AgentConfig::load();
    let pname = AiWorker::provider_name(&cfg, provider_override);
    let worker = AiWorker::start(cfg.clone(), provider_override.map(|s| s.to_string()));
    let wake_msg = single_instance::wake_message_id();

    unsafe {
        let instance =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PetAgentInputWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(instance.0),
            lpszClassName: class,
            hbrBackground: HBRUSH(GetStockObject(GET_STOCK_OBJECT_FLAGS(0)).0), // NULL_BRUSH
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            log_line("RegisterClassExW failed");
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class,
            w!("DesktopPet Agent"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            200,
            200,
            380,
            150,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .unwrap_or_default();

        // 诊断：确认创建时的实际尺寸
        {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            log_line(&format!(
                "window created: {}x{} at ({},{})",
                rc.right - rc.left,
                rc.bottom - rc.top,
                rc.left,
                rc.top
            ));
        }
        // 强制目标尺寸（不激活、不改 Z 序）
        let _ = SetWindowPos(
            hwnd,
            None,
            200,
            200,
            380,
            150,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        create_controls(hwnd);

        CTX.with(|c| {
            *c.borrow_mut() = Some(UiContext {
                worker,
                cfg: cfg.clone(),
                generation: 0,
                active_gen: None,
                thinking: false,
                wake_msg,
                tray_added: false,
                last_reply: String::new(),
                provider_name: pname,
                conv: crate::context::Conversation::new(&cfg),
                pending_user: None,
                tts: crate::tts::TtsController::new(&cfg),
            });
        });

        // 托盘 + 快捷键
        setup_tray(hwnd);
        setup_hotkey(hwnd);

        set_status(hwnd, &format!("{} · Ready", provider_status_label()));

        // 轮询 worker 结果的定时器（每 100ms）
        let _ = SetTimer(Some(hwnd), POLL_TIMER_ID, 100, None);

        // 初始显示
        let _ = ShowWindow(hwnd, SW_SHOW);
        position_near_pet(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&mut msg);
        }

        remove_tray(hwnd);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        log_line("ui loop exited");
    }
}

fn provider_status_label() -> String {
    CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| ctx.provider_name.clone())
            .unwrap_or_else(|| "mock".into())
    })
}

unsafe fn create_controls(hwnd: HWND) {
    let instance =
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
    let hinst = HINSTANCE(instance.0);

    // EDIT（多行）
    let _ = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        w!("EDIT"),
        w!(""),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
        10,
        10,
        340,
        70,
        Some(hwnd),
        Some(HMENU(ID_EDIT as isize as *mut _)),
        Some(hinst),
        None,
    );

    // 发送按钮
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("BUTTON"),
        w!("发送 (Enter)"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        10,
        86,
        120,
        28,
        Some(hwnd),
        Some(HMENU(ID_SEND as isize as *mut _)),
        Some(hinst),
        None,
    );

    // 状态文字
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("STATIC"),
        w!(""),
        WS_CHILD | WS_VISIBLE,
        140,
        92,
        220,
        20,
        Some(hwnd),
        Some(HMENU(ID_STATUS as isize as *mut _)),
        Some(hinst),
        None,
    );
}

unsafe fn setup_hotkey(hwnd: HWND) {
    // Ctrl + Alt + Space
    let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
    let ok = RegisterHotKey(Some(hwnd), HOTKEY_ID, mods, VK_SPACE.0 as u32);
    if ok.is_err() {
        log_line("RegisterHotKey(Ctrl+Alt+Space) failed (maybe already in use)");
    } else {
        log_line("hotkey registered: Ctrl+Alt+Space");
    }
}

unsafe fn setup_tray(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = TRAY_CALLBACK;
    nid.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
    let tip: Vec<u16> = "DesktopPet Agent".encode_utf16().collect();
    for (i, c) in tip.iter().enumerate().take(127) {
        nid.szTip[i] = *c;
    }
    if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
        CTX.with(|c| {
            if let Some(ctx) = c.borrow_mut().as_mut() {
                ctx.tray_added = true;
            }
        });
    }
}

unsafe fn remove_tray(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn get_text(hwnd: HWND, id: i32) -> String {
    let ctrl = GetDlgItem(Some(hwnd), id).unwrap_or_default();
    let len = GetWindowTextLengthW(ctrl);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let n = GetWindowTextW(ctrl, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize])
}

unsafe fn set_text(hwnd: HWND, id: i32, text: &str) {
    let ctrl = GetDlgItem(Some(hwnd), id).unwrap_or_default();
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = SetWindowTextW(ctrl, PCWSTR(wide.as_ptr()));
}

unsafe fn set_status(hwnd: HWND, text: &str) {
    set_text(hwnd, ID_STATUS, text);
}

unsafe fn toggle_send_button(hwnd: HWND, thinking: bool) {
    let ctrl = GetDlgItem(Some(hwnd), ID_SEND).unwrap_or_default();
    let label = if thinking { "停止" } else { "发送 (Enter)" };
    let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = SetWindowTextW(ctrl, PCWSTR(wide.as_ptr()));
}

unsafe fn position_near_pet(hwnd: HWND) {
    // 尝试查询桌宠位置
    let pet = ipc::query_status();
    let (x, y) = match pet {
        Some(s) if s.online => {
            let [l, t, r, b] = s.window_rect;
            let w = 380;
            let h = 150;
            // 优先桌宠上方
            let mut x = l + (r - l) / 2 - w / 2;
            let mut y = t - h - 8;
            if y < 0 {
                y = b + 8;
            }
            if x < 0 {
                x = 8;
            }
            (x, y)
        }
        _ => {
            // 无桌宠：鼠标所在显示器中央
            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut pt);
            (pt.x - 190, pt.y - 75)
        }
    };
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        x,
        y,
        0,
        0,
        SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
}

unsafe fn focus_input(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_SHOW);
    position_near_pet(hwnd);
    let _ = SetForegroundWindow(hwnd);
    let ctrl = GetDlgItem(Some(hwnd), ID_EDIT).unwrap_or_default();
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(ctrl));
}

unsafe fn send_current(hwnd: HWND) {
    let text = get_text(hwnd, ID_EDIT);
    if text.trim().is_empty() {
        return;
    }
    // 清空输入框
    set_text(hwnd, ID_EDIT, "");
    submit_text(hwnd, &text);
}

unsafe fn submit_text(hwnd: HWND, text: &str) {
    // 阶段 1：仅在短借用内更新状态并提交请求（不做任何 Win32 调用）
    let (ok, pname, already) = CTX.with(|c| {
        let mut b = c.borrow_mut();
        let ctx = match b.as_mut() {
            Some(x) => x,
            None => return (false, String::new(), true),
        };
        if ctx.thinking {
            return (true, ctx.provider_name.clone(), true);
        }
        ctx.generation += 1;
        let gen = ctx.generation;
        ctx.active_gen = Some(gen);
        ctx.thinking = true;

        // 短期多轮：system + history + 本轮 user
        let mut msgs = ctx.conv.messages();
        msgs.push(crate::context::ChatMessage {
            role: "user",
            content: text.to_string(),
        });
        ctx.pending_user = Some((gen, text.to_string()));
        let req = AiRequest {
            generation: gen,
            messages: msgs,
            user_text: text.to_string(),
        };
        let ok = ctx.worker.submit(req);
        if !ok {
            ctx.thinking = false;
            ctx.active_gen = None;
        }
        (ok, ctx.provider_name.clone(), false)
    });

    if already {
        return;
    }
    // 阶段 2：Win32 调用（不持有任何借用）
    if !ok {
        set_status(hwnd, "内部错误：worker 不可用");
        return;
    }
    set_status(hwnd, &format!("{pname} · Thinking…"));
    toggle_send_button(hwnd, true);
    log_line(&format!(
        "submitted request ({} chars)",
        text.chars().count()
    ));
}

/// 轮询 worker 结果（由定时器驱动）。
unsafe fn poll_results(hwnd: HWND) {
    let result = CTX.with(|c| c.borrow().as_ref().and_then(|ctx| ctx.worker.try_recv()));
    if let Some(res) = result {
        let (is_active, pname) = CTX.with(|c| {
            let b = c.borrow();
            let ctx = b.as_ref().unwrap();
            (
                ctx.active_gen == Some(res.generation) || ctx.active_gen.is_none(),
                ctx.provider_name.clone(),
            )
        });
        // 无论是否 active，都要复位 thinking 状态（仅当是当前 active）
        CTX.with(|c| {
            if let Some(ctx) = c.borrow_mut().as_mut() {
                if ctx.active_gen == Some(res.generation) {
                    ctx.thinking = false;
                    ctx.active_gen = None;
                    ctx.last_reply = res.outcome.clone().unwrap_or_default();
                }
            }
        });
        toggle_send_button(hwnd, false);

        if !is_active {
            log_line(&format!("discarded stale result (gen {})", res.generation));
            return;
        }

        match res.outcome {
            Ok(reply) => {
                log_line(&format!("provider replied in {}ms", res.elapsed_ms));
                set_status(hwnd, &format!("{pname} · Ready"));
                // 写入短期 history（仅成功轮）
                CTX.with(|c| {
                    if let Some(ctx) = c.borrow_mut().as_mut() {
                        if let Some((g, u)) = ctx.pending_user.clone() {
                            if g == res.generation {
                                ctx.conv.push_user(&u);
                                ctx.conv.push_assistant(&reply);
                                ctx.pending_user = None;
                            }
                        }
                    }
                });
                // 发送到桌宠（截断 + 映射）
                let bubble = truncate(&reply, 80);
                let mapped = mapper::map(&bubble);
                match ipc::send_expression(
                    Some(&mapped.text),
                    mapped.motion,
                    pet_protocol::Priority::Normal,
                    None,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        log_line(&format!("ipc error: {e}"));
                        set_status(hwnd, &format!("{pname} · 桌宠当前未运行"));
                    }
                }
                // TTS：朗读与气泡相同的短文本（若启用）
                let speech = crate::tts::sanitize::sanitize(&reply, mapped.text.len().max(1));
                CTX.with(|c| {
                    if let Some(ctx) = c.borrow_mut().as_mut() {
                        ctx.tts.speak(&speech);
                    }
                });
            }
            Err(e) => {
                log_line(&format!("provider error: {e}"));
                set_status(hwnd, &format!("{pname} · 模型暂时无法连接"));
            }
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    let cleaned = text.replace(['\r', '\n'], " ").trim().to_string();
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let mut s: String = cleaned.chars().take(max).collect();
        s.push('…');
        s
    }
}

unsafe fn cancel_current(hwnd: HWND) {
    CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.active_gen = None; // 结果回来时会被丢弃
            ctx.thinking = false;
            ctx.pending_user = None;
            ctx.tts.stop();
        }
    });
    toggle_send_button(hwnd, false);
    set_status(hwnd, &format!("{} · Ready", provider_status_label()));
    log_line("request cancelled (result will be discarded)");
}

unsafe fn clear_conversation(hwnd: HWND) {
    CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.conv.clear();
            ctx.pending_user = None;
            ctx.tts.stop();
        }
    });
    set_status(hwnd, &format!("{} · 对话已清空", provider_status_label()));
    log_line("conversation cleared (short-term only)");
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 唤醒消息（另一实例请求显示输入框）
    let wake = CTX.with(|c| c.borrow().as_ref().map(|ctx| ctx.wake_msg).unwrap_or(0));
    if wake != 0 && msg == wake {
        focus_input(hwnd);
        return LRESULT(0);
    }

    match msg {
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID {
                if IsWindowVisible(hwnd).as_bool() {
                    focus_input(hwnd);
                } else {
                    focus_input(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if id == ID_SEND && code == 0 {
                // 先取出 thinking 状态（借用立即释放），避免与后续 borrow_mut 冲突
                let thinking =
                    CTX.with(|c| c.borrow().as_ref().map(|ctx| ctx.thinking).unwrap_or(false));
                if thinking {
                    cancel_current(hwnd);
                } else {
                    send_current(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            if vk == 0x0D {
                // Enter
                let shift = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0; // VK_SHIFT
                if !shift {
                    send_current(hwnd);
                    return LRESULT(0);
                }
                // Shift+Enter → 换行（默认行为）
            } else if vk == 0x1B {
                // Esc → 隐藏
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        TRAY_CALLBACK => {
            let ev = lparam.0 as u32;
            match ev {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => focus_input(hwnd),
                WM_RBUTTONUP => {
                    let cmd = show_tray_menu(hwnd);
                    match cmd {
                        CMD_OPEN => focus_input(hwnd),
                        CMD_CLEAR => clear_conversation(hwnd),
                        CMD_AUTOSTART => {
                            let now = autostart::toggle();
                            log_line(&format!("agent autostart toggled: {now}"));
                        }
                        CMD_EXIT => {
                            let _ = DestroyWindow(hwnd);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            poll_results(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            // 关闭按钮 → 隐藏而非退出
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_tray_menu(hwnd: HWND) -> usize {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("打开输入框"));
    let _ = AppendMenuW(menu, MF_STRING, CMD_CLEAR, w!("清空对话"));
    let as_flags = if autostart::is_enabled() {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(menu, as_flags, CMD_AUTOSTART, w!("开机启动"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
    let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("退出"));

    let mut pt = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    cmd.0 as usize
}

/// 定时器 ID（由外部在 run_ui 中启动）。
pub const POLL_TIMER_ID: usize = 1;
