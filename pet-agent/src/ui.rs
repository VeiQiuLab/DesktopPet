//! pet-agent 原生 Win32 输入窗口 + 全局快捷键 + 托盘 + 常驻消息循环。
//!
//! 线程边界：
//! - UI 线程：窗口 / 控件 / 消息循环 / 托盘 / 快捷键；只与 worker 通过 channel 交互。
//! - AI worker 线程：HTTP 请求（见 worker.rs）。
//! - LLM 请求绝不阻塞 UI 线程。

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, ReleaseCapture, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_SPACE,
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
const CMD_MEMORY_MGMT: usize = 6;
const CMD_PERSONA_BASE: usize = 50;

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
    #[allow(dead_code)]
    tray_added: bool,
    last_reply: String,
    provider_name: String,
    /// 短期多轮上下文（与 chat 模式同一套 Conversation）。
    conv: crate::context::Conversation,
    /// 待写入 history 的 (generation, user_text)。
    pending_user: Option<(u64, String)>,
    /// TTS 控制器。
    tts: crate::tts::TtsController,
    /// Persona。
    persona: crate::persona::Persona,
    /// Memory（None = 不可用，降级无记忆模式）。
    memory: Option<crate::memory::MemoryManager>,
    /// 待确认反馈（如「已加入记忆」）。
    toast: Option<String>,
}

/// 守护父进程：等 desktop-pet 退出后自动退出本进程。
fn spawn_parent_guard() {
    let pid = crate::PARENT_PID.load(std::sync::atomic::Ordering::Relaxed);
    if pid == 0 {
        return;
    }
    std::thread::Builder::new()
        .name("parent-guard".into())
        .spawn(move || unsafe {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::{
                OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
            };
            let h = match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                Ok(h) => h,
                Err(_) => return,
            };
            // 阻塞直到父进程退出（含被强杀）
            let _ = WaitForSingleObject(h, INFINITE);
            let _ = CloseHandle(h);
            crate::log_line("parent process exited, shutting down agent");
            single_instance::broadcast_quit();
            // 兜底：若广播未生效（例如窗口未创建），直接退出
            std::thread::sleep(std::time::Duration::from_millis(1500));
            std::process::exit(0);
        })
        .ok();
}

pub fn run_ui(provider_override: Option<&str>) {
    // panic hook：把 panic 写入日志，避免消息循环静默崩溃
    std::panic::set_hook(Box::new(|info| {
        crate::log_line(&format!("PANIC: {info}"));
    }));
    let cfg = AgentConfig::load();
    crate::config::set_lip_sync_enabled(cfg.lip_sync.enabled);
    spawn_parent_guard();
    crate::tts::sapi::cleanup_temp();
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
            hbrBackground: crate::theme::bg_brush(), // 深色胶囊底
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            log_line("RegisterClassExW failed");
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class,
            w!("DesktopPet Agent"),
            WS_POPUP,
            200,
            200,
            428,
            64,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .unwrap_or_default();

        // 现代外观：大圆角 + 亚克力玻璃背景
        apply_modern_style(hwnd);

        let _ = SetWindowPos(hwnd, None, 200, 200, 428, 64, SWP_NOZORDER | SWP_NOACTIVATE);
        // 胶囊形窗口区域（圆角 = 高度一半）
        {
            use windows::Win32::Graphics::Gdi::CreateRoundRectRgn;
            use windows::Win32::Graphics::Gdi::SetWindowRgn;
            let rgn = CreateRoundRectRgn(0, 0, 429, 65, 64, 64);
            let _ = SetWindowRgn(hwnd, Some(rgn), true);
        }
        create_controls(hwnd);
        // 深色主题字体
        {
            let dpi = {
                let d = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
                if d == 0 {
                    96
                } else {
                    d
                }
            };
            let font = crate::theme::create_font(dpi, 12);
            for id in [ID_EDIT, ID_SEND, ID_STATUS] {
                if let Ok(c) = GetDlgItem(Some(hwnd), id) {
                    crate::theme::apply_font(c, font);
                }
            }
        }

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
                persona: crate::persona::Persona::load_or_default(&cfg.active_persona),
                memory: open_memory(),
                toast: None,
            });
        });

        // 托盘 + 快捷键
        // 托盘合并到 desktop-pet：agent 不再显示自己的托盘图标
        // setup_tray(hwnd);
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

/// 打开 memory DB（失败降级为 None）。
fn open_memory() -> Option<crate::memory::MemoryManager> {
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    p.push("data");
    p.push("memory.db");
    match crate::memory::MemoryManager::open(&p) {
        Ok(m) => Some(m),
        Err(e) => {
            log_line(&format!(
                "memory unavailable: {e} (degraded to no-memory mode)"
            ));
            None
        }
    }
}

/// 切换 Persona：更新配置 + 清空短期对话（不动 Memory/Character/TTS）。
fn switch_persona(id: &str) {
    let mut cfg = AgentConfig::load();
    cfg.active_persona = id.to_string();
    cfg.save();
    CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.persona = crate::persona::Persona::load_or_default(id);
            ctx.conv.clear();
        }
    });
    log_line(&format!(
        "persona switched to '{id}' (conversation cleared, memory kept)"
    ));
}

/// Persona 选择（光标处弹出菜单）。
unsafe fn open_persona_dialog(hwnd: HWND) {
    let menu = CreatePopupMenu().unwrap_or_default();
    let current = CTX.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| ctx.persona.id.clone())
            .unwrap_or_default()
    });
    for (i, p) in crate::persona::scan().iter().enumerate() {
        let mut flags = MF_STRING;
        if p.id == current {
            flags |= MF_CHECKED;
        }
        let label = wide_str(&format!("{} ({})", p.name, p.id));
        let _ = AppendMenuW(menu, flags, i + 1, PCWSTR(label.as_ptr()));
    }
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
    let idx = cmd.0 as usize;
    if idx >= 1 {
        let personas = crate::persona::scan();
        if let Some(p) = personas.get(idx - 1) {
            switch_persona(&p.id);
        }
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

    // 输入区（无边框单行/多行，与外层同底色 → 无「两层」观感）
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        w!(""),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
        26,
        16,
        352,
        32,
        Some(hwnd),
        Some(HMENU(ID_EDIT as isize as *mut _)),
        Some(hinst),
        None,
    );

    // 内嵌圆形发送键（胶囊右侧）
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("BUTTON"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
        378,
        14,
        40,
        40,
        Some(hwnd),
        Some(HMENU(ID_SEND as isize as *mut _)),
        Some(hinst),
        None,
    );

    // 状态控件（隐藏：仅内部用于反馈，不显示技术信息）
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("STATIC"),
        w!(""),
        WS_CHILD,
        0,
        0,
        0,
        0,
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

#[allow(dead_code)]
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
            let w = 480;
            let h = 240;
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

        // 显式「记住……」→ 直接写入 active（有审计日志）；冲突则进入 pending
        if let Some(mem) = ctx.memory.as_ref() {
            if let Some(content) = crate::memory::intent::detect_explicit(text) {
                let kind = crate::memory::intent::suggest_kind(&content);
                if let Some(existing) = mem.find_conflict(&content) {
                    let _ =
                        mem.propose("update", kind, &content, Some(existing.id), "user_explicit");
                    ctx.toast = Some("已记录为待确认修改。".into());
                } else {
                    match mem.create(kind, &content, "user_explicit") {
                        Ok(_) => ctx.toast = Some("已加入记忆。".into()),
                        Err(e) => log_line(&format!("memory create failed: {e}")),
                    }
                }
            } else if let Some(new_val) = crate::memory::intent::detect_change(text) {
                // 显式修改意图：优先按新值找冲突项，否则回退最近一条 active
                let target = mem
                    .find_conflict(&new_val)
                    .or_else(|| mem.most_recent_active());
                if let Some(existing) = target {
                    let kind = crate::memory::intent::suggest_kind(&existing.content);
                    let _ =
                        mem.propose("update", kind, &new_val, Some(existing.id), "user_explicit");
                    ctx.toast = Some("已记录为待确认修改。".into());
                }
            } else if crate::memory::intent::looks_memorable(text) {
                let kind = crate::memory::intent::suggest_kind(text);
                let _ = mem.propose("create", kind, text, None, "implicit");
            }
        }
        // Prompt：Persona → Memory → 历史 → 当前 user
        let memories = ctx
            .memory
            .as_ref()
            .map(|m| m.retrieve(text, 12, 1200))
            .unwrap_or_default();
        let msgs =
            crate::prompt::PromptBuilder::build(&ctx.persona, &memories, &ctx.conv.history(), text);
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
    let toast = CTX.with(|c| c.borrow_mut().as_mut().and_then(|ctx| ctx.toast.take()));
    match toast {
        Some(t) => set_status(hwnd, &t),
        None => set_status(hwnd, &format!("{pname} · Thinking…")),
    }
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
    // 由 desktop-pet 托盘广播的指令
    {
        static ONCE: std::sync::OnceLock<(u32, u32, u32)> = std::sync::OnceLock::new();
        let ids = ONCE.get_or_init(|| {
            (
                single_instance::named_message_id("DesktopPetAgent_OpenMemory"),
                single_instance::named_message_id("DesktopPetAgent_OpenPersona"),
                single_instance::named_message_id("DesktopPetAgent_Quit"),
            )
        });
        if ids.0 != 0 && msg == ids.0 {
            if let Some(m) = open_memory() {
                crate::memory_ui::show(m);
            }
            return LRESULT(0);
        }
        if ids.1 != 0 && msg == ids.1 {
            open_persona_dialog(hwnd);
            return LRESULT(0);
        }
        if ids.2 != 0 && msg == ids.2 {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
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
                        CMD_MEMORY_MGMT => {
                            if let Some(m) = open_memory() {
                                crate::memory_ui::show(m);
                            }
                        }
                        c if c >= CMD_PERSONA_BASE => {
                            let personas = crate::persona::scan();
                            if let Some(p) = personas.get(c - CMD_PERSONA_BASE) {
                                switch_persona(&p.id);
                                set_status(hwnd, &format!("Persona -> {}", p.name));
                            }
                        }
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
        // 无边框窗口：按住空白处可拖动
        WM_LBUTTONDOWN => {
            let _ = ReleaseCapture();
            let _ = SendMessageW(
                hwnd,
                WM_NCLBUTTONDOWN,
                Some(WPARAM(HTCAPTION as usize)),
                Some(LPARAM(0)),
            );
            LRESULT(0)
        }
        WM_DRAWITEM => {
            draw_send_button(lparam);
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT => {
            crate::theme::on_ctlcolor(hwnd, msg, wparam)
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

/// 字符串 → nul 结尾 UTF-16。
/// 应用 Win11 现代外观：大圆角 + 亚克力玻璃背景。
unsafe fn apply_modern_style(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows::Win32::UI::Controls::MARGINS;
    let round = DWMWCP_ROUND;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &round as *const _ as *const core::ffi::c_void,
        4,
    );
    // 不设置 SYSTEMBACKDROP_TYPE（会覆盖自绘深色背景）
    // 不扩展 DWM 框架（避免与自绘背景冲突）
    let _ = std::mem::size_of::<MARGINS>();
    // 注：暂不启用亚克力，避免 EDIT 子控件与半透明玻璃产生「两层颜色」。
    // 真液态玻璃（WindowsLiquidGlass / D3D11 自绘）将在后续集成。
}

/// 通过 SetWindowCompositionAttribute 应用亚克力（毛玻璃）。
#[allow(dead_code)]
unsafe fn apply_acrylic(hwnd: HWND) {
    #[repr(C)]
    struct AccentPolicy {
        accent_state: i32,
        accent_flags: i32,
        gradient_color: u32,
        animation_id: i32,
    }
    #[repr(C)]
    struct WcaData {
        attribute: i32,
        data: *mut core::ffi::c_void,
        size_of_data: usize,
    }
    type SetWca = unsafe extern "system" fn(HWND, *mut WcaData) -> i32;

    let user32 = windows::Win32::System::LibraryLoader::GetModuleHandleW(w!("user32.dll"))
        .unwrap_or_default();
    let addr = windows::Win32::System::LibraryLoader::GetProcAddress(
        user32,
        windows::core::s!("SetWindowCompositionAttribute"),
    );
    let f: SetWca = match addr {
        Some(a) => std::mem::transmute(a),
        None => return,
    };
    // ACCENT_ENABLE_ACRYLICBLURBEHIND = 4；渐变色 AABBGGRR（深色半透明）
    let mut policy = AccentPolicy {
        accent_state: 4,
        accent_flags: 2,
        gradient_color: 0xCC1A1A1A,
        animation_id: 0,
    };
    let mut data = WcaData {
        attribute: 19, // WCA_ACCENT_POLICY
        data: &mut policy as *mut _ as *mut core::ffi::c_void,
        size_of_data: std::mem::size_of::<AccentPolicy>(),
    };
    let _ = f(hwnd, &mut data);
}

/// 自绘圆形发送键。
unsafe fn draw_send_button(lparam: LPARAM) {
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, SelectObject, SetBkMode, SetTextColor,
        HGDIOBJ, TRANSPARENT,
    };
    use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
    let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
    let hdc = dis.hDC;
    let r = dis.rcItem;
    let bg = windows::Win32::Foundation::COLORREF(0x00F0_7030);
    let brush = CreateSolidBrush(bg);
    let old = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = Ellipse(hdc, r.left, r.top, r.right, r.bottom);
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    use windows::Win32::Graphics::Gdi::{CreatePen, LineTo, MoveToEx, PS_SOLID};
    let pen = CreatePen(
        PS_SOLID,
        3,
        windows::Win32::Foundation::COLORREF(0x00FFFFFF),
    );
    let oldp = SelectObject(hdc, HGDIOBJ(pen.0));
    let cx = (r.left + r.right) / 2;
    let cy = (r.top + r.bottom) / 2;
    let _ = MoveToEx(hdc, cx - 7, cy - 8, None);
    let _ = LineTo(hdc, cx + 8, cy);
    let _ = LineTo(hdc, cx - 7, cy + 8);
    let _ = LineTo(hdc, cx - 7, cy - 8);
    let _ = SelectObject(hdc, oldp);
    let _ = DeleteObject(HGDIOBJ(pen.0));
    let _ = (DrawTextW, SetBkMode, SetTextColor, TRANSPARENT);
    let mut t: Vec<u16> = "".encode_utf16().collect();
    let mut rc = r;
    let _ = DrawTextW(
        hdc,
        &mut t,
        &mut rc,
        windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(0x25),
    );
}

fn wide_str(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn show_tray_menu(hwnd: HWND) -> usize {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, w!("打开输入框"));
    let _ = AppendMenuW(menu, MF_STRING, CMD_CLEAR, w!("清空对话"));
    // 记忆 / Persona（只读展示；管理走 CLI）
    let (pending_n, persona_name, mem_ok) = CTX.with(|c| {
        let b = c.borrow();
        let ctx = b.as_ref().unwrap();
        let pn = ctx
            .memory
            .as_ref()
            .map(|m| m.list_pending().len())
            .unwrap_or(0);
        (pn, ctx.persona.name.clone(), ctx.memory.is_some())
    });
    if mem_ok {
        let label = wide_str(&format!("记忆管理 ({pending_n} 待确认)"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_MEMORY_MGMT, PCWSTR(label.as_ptr()));
    } else {
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, w!("记忆不可用"));
    }
    let psub = CreatePopupMenu().unwrap_or_default();
    for (i, p) in crate::persona::scan().iter().enumerate() {
        let mut flags = MF_STRING;
        if p.name == persona_name {
            flags |= MF_CHECKED;
        }
        let label = wide_str(&format!("{} ({})", p.name, p.id));
        let _ = AppendMenuW(psub, flags, CMD_PERSONA_BASE + i, PCWSTR(label.as_ptr()));
    }
    let _ = AppendMenuW(menu, MF_POPUP, psub.0 as usize, w!("Persona"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
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
