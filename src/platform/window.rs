//! Win32 透明置顶窗口 + 消息处理 + 命中测试 + 拖拽 + 右键菜单 + 托盘回调。
//!
//! - WS_EX_NOREDIRECTIONBITMAP + DirectComposition 合成
//! - WM_NCHITTEST 命中测试（模型区域 HTCLIENT，透明区域 HTTRANSPARENT）
//! - 拖拽仅命中模型时启动
//! - WM_RBUTTONUP / 托盘右键弹出菜单，菜单选择经 channel 上报
//! - 本层只采集输入 / 处理窗口级命令，不直接调用 Cubism motion

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, ScreenToClient, UpdateWindow, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::autostart;
use crate::behavior::PetEvent;
use crate::character::cubism;
use crate::config::{log_line, Config};
use crate::platform::tray::{
    show_tray_menu, TrayIcon, CMD_CHAR_BASE, CMD_TRAY_AUTOSTART, CMD_TRAY_EXIT, CMD_TRAY_RESET,
    CMD_TRAY_TOGGLE_VISIBLE, TRAY_CALLBACK_MSG,
};

/// 右键菜单命令 ID。
const CMD_NOD: usize = 1;
const CMD_SHAKE: usize = 2;
const CMD_RESET: usize = 3;
const CMD_QUIT: usize = 4;

const DRAG_THRESHOLD: i32 = 5;
const DOUBLE_CLICK_MS: u128 = 400;

pub struct WindowParams {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct WindowState {
    pub hwnd: HWND,
    pub dragging: bool,
    pub drag_moved: bool,
    pub drag_origin: POINT,
    pub window_origin: POINT,
    pub model_handle: *mut c_void,
    pub client_size: (i32, i32),
    pub events: Sender<PetEvent>,
    pub last_click: Option<Instant>,
    pub hovering: bool,
    pub config: Config,
    /// 持有托盘图标；Drop 时自动移除。字段本身不直接读取。
    #[allow(dead_code)]
    pub tray: Option<TrayIcon>,
    pub characters: Vec<(String, String)>,
    pub active_character: String,
    pub auto_start: bool,
    pub menu_open: bool,
}

pub unsafe fn create_window(
    params: &WindowParams,
    config: Config,
    events: Sender<PetEvent>,
    characters: Vec<(String, String)>,
    active_character: String,
) -> windows::core::Result<HWND> {
    let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;

    let class_name = w!("DesktopPetWindow");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: class_name,
        ..Default::default()
    };
    if RegisterClassExW(&wc) == 0 {
        log_line("RegisterClassExW failed");
    }

    let ex_style = WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW;
    let style = WS_POPUP;

    let hwnd = CreateWindowExW(
        ex_style,
        class_name,
        w!("Desktop Pet"),
        style,
        params.x,
        params.y,
        params.width,
        params.height,
        None,
        None,
        Some(HINSTANCE(instance.0)),
        None,
    )?;

    let tray = TrayIcon::new(hwnd);
    let auto_start = autostart::is_enabled();

    let state = Box::new(WindowState {
        hwnd,
        dragging: false,
        drag_moved: false,
        drag_origin: POINT::default(),
        window_origin: POINT::default(),
        model_handle: std::ptr::null_mut(),
        client_size: (params.width, params.height),
        events,
        last_click: None,
        hovering: false,
        config,
        tray,
        characters,
        active_character,
        auto_start,
        menu_open: false,
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = UpdateWindow(hwnd);
    log_line("window created");
    Ok(hwnd)
}

pub unsafe fn set_model(hwnd: HWND, handle: *mut c_void) {
    let st = state_ptr(hwnd);
    if !st.is_null() {
        (*st).model_handle = handle;
    }
}

pub unsafe fn set_active_character(hwnd: HWND, id: &str) {
    let st = state_ptr(hwnd);
    if !st.is_null() {
        (*st).active_character = id.to_string();
    }
}

pub unsafe fn is_menu_open(hwnd: HWND) -> bool {
    let st = state_ptr(hwnd);
    if st.is_null() {
        return false;
    }
    (*st).menu_open
}

pub unsafe fn is_dragging(hwnd: HWND) -> bool {
    let st = state_ptr(hwnd);
    if st.is_null() {
        return false;
    }
    (*st).dragging
}

unsafe fn state_ptr(hwnd: HWND) -> *mut WindowState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState
}

unsafe fn hit_model_at_screen(st: *mut WindowState, screen_x: i32, screen_y: i32) -> bool {
    if st.is_null() || (*st).model_handle.is_null() {
        return false;
    }
    let hwnd = (*st).hwnd;
    let mut pt = POINT {
        x: screen_x,
        y: screen_y,
    };
    let _ = ScreenToClient(hwnd, &mut pt);
    let (w, h) = (*st).client_size;
    cubism::hit_test_raw(
        (*st).model_handle,
        pt.x as f32,
        pt.y as f32,
        w as f32,
        h as f32,
    )
}

/// 追加字符串菜单项（辅助）。
unsafe fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
}

/// 桌宠本体右键菜单。
unsafe fn show_context_menu(hwnd: HWND) -> usize {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };
    append(menu, MF_STRING, CMD_NOD, "点头");
    append(menu, MF_STRING, CMD_SHAKE, "摇头");
    append(menu, MF_SEPARATOR, 0, "");
    append(menu, MF_STRING, CMD_RESET, "重置位置");
    append(menu, MF_STRING, CMD_QUIT, "退出");

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
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

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let st = state_ptr(hwnd);
            if !st.is_null() {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                if hit_model_at_screen(st, pt.x, pt.y) {
                    return LRESULT(HTCLIENT as isize);
                }
                return LRESULT(HTTRANSPARENT as isize);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let st = state_ptr(hwnd);
            if !st.is_null() {
                if (*st).dragging {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let dx = pt.x - (*st).drag_origin.x;
                    let dy = pt.y - (*st).drag_origin.y;
                    if !(*st).drag_moved && (dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD)
                    {
                        (*st).drag_moved = true;
                        let _ = (*st).events.send(PetEvent::DragStart);
                    }
                    let nx = (*st).window_origin.x + dx;
                    let ny = (*st).window_origin.y + dy;
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        nx,
                        ny,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                } else {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let hit = hit_model_at_screen(st, pt.x, pt.y);
                    if hit && !(*st).hovering {
                        (*st).hovering = true;
                        let _ = (*st).events.send(PetEvent::PointerEnter);
                    } else if !hit && (*st).hovering {
                        (*st).hovering = false;
                        let _ = (*st).events.send(PetEvent::PointerLeave);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let st = state_ptr(hwnd);
            if !st.is_null() {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                if !hit_model_at_screen(st, pt.x, pt.y) {
                    return LRESULT(HTTRANSPARENT as isize);
                }
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                (*st).dragging = true;
                (*st).drag_moved = false;
                (*st).drag_origin = pt;
                (*st).window_origin = POINT {
                    x: rect.left,
                    y: rect.top,
                };
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let st = state_ptr(hwnd);
            if !st.is_null() && (*st).dragging {
                (*st).dragging = false;
                let _ = ReleaseCapture();

                if (*st).drag_moved {
                    let _ = (*st).events.send(PetEvent::DragEnd);
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    (*st).config.window.x = rect.left;
                    (*st).config.window.y = rect.top;
                    (*st).config.save();
                } else {
                    let now = Instant::now();
                    let is_double = match (*st).last_click {
                        Some(t) => now.duration_since(t).as_millis() < DOUBLE_CLICK_MS,
                        None => false,
                    };
                    if is_double {
                        (*st).last_click = None;
                        let _ = (*st).events.send(PetEvent::DoubleClick);
                    } else {
                        (*st).last_click = Some(now);
                        let _ = (*st).events.send(PetEvent::LeftClick);
                    }
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let st = state_ptr(hwnd);
            if !st.is_null() {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                if hit_model_at_screen(st, pt.x, pt.y) {
                    let _ = (*st).events.send(PetEvent::RightClick);
                    (*st).menu_open = true;
                    let cmd = show_context_menu(hwnd);
                    (*st).menu_open = false;
                    let ev = match cmd {
                        CMD_NOD => Some(PetEvent::MenuNod),
                        CMD_SHAKE => Some(PetEvent::MenuShake),
                        CMD_RESET => Some(PetEvent::MenuReset),
                        CMD_QUIT => Some(PetEvent::MenuQuit),
                        _ => None,
                    };
                    if let Some(e) = ev {
                        let _ = (*st).events.send(e);
                    }
                }
            }
            LRESULT(0)
        }
        TRAY_CALLBACK_MSG => {
            // 托盘鼠标事件在 lparam
            let ev = lparam.0 as u32;
            let st = state_ptr(hwnd);
            if st.is_null() {
                return LRESULT(0);
            }
            match ev {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    // 左键单击切换可见性
                    toggle_visibility(hwnd);
                }
                WM_RBUTTONUP => {
                    let characters = (*st).characters.clone();
                    let active = (*st).active_character.clone();
                    let auto_start = (*st).auto_start;
                    (*st).menu_open = true;
                    let cmd = show_tray_menu(hwnd, &characters, &active, auto_start);
                    (*st).menu_open = false;
                    match cmd {
                        CMD_TRAY_TOGGLE_VISIBLE => toggle_visibility(hwnd),
                        CMD_TRAY_RESET => {
                            let _ = (*st).events.send(PetEvent::MenuReset);
                        }
                        CMD_TRAY_AUTOSTART => {
                            let now = autostart::toggle();
                            (*st).auto_start = now;
                            log_line(&format!("auto start toggled: {now}"));
                        }
                        CMD_TRAY_EXIT => {
                            let _ = (*st).events.send(PetEvent::MenuQuit);
                        }
                        c if c >= CMD_CHAR_BASE => {
                            let idx = c - CMD_CHAR_BASE;
                            if let Some((id, _)) = characters.get(idx) {
                                let _ =
                                    (*st).events.send(PetEvent::TraySwitchCharacter(id.clone()));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x1B {
                // ESC → 统一退出
                let st = state_ptr(hwnd);
                if !st.is_null() {
                    let _ = (*st).events.send(PetEvent::MenuQuit);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // 统一退出：发事件，由主循环销毁窗口
            let st = state_ptr(hwnd);
            if !st.is_null() {
                let _ = (*st).events.send(PetEvent::MenuQuit);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let st = state_ptr(hwnd);
            if !st.is_null() {
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                (*st).config.window.x = rect.left;
                (*st).config.window.y = rect.top;
                (*st).config.save();
                // 回收 Box（TrayIcon 在 Drop 时移除图标）
                drop(Box::from_raw(st));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 桌宠窗口的屏幕矩形（供气泡定位）。
pub unsafe fn pet_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        Some(rect)
    } else {
        None
    }
}

/// 桌宠所在显示器的工作区（排除任务栏）。
pub unsafe fn monitor_work_area(hwnd: HWND) -> Option<crate::presentation::controller::Rect> {
    let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if hmon.0.is_null() {
        return None;
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
        return None;
    }
    Some(crate::presentation::controller::Rect {
        left: mi.rcWork.left,
        top: mi.rcWork.top,
        right: mi.rcWork.right,
        bottom: mi.rcWork.bottom,
    })
}

/// 切换窗口可见性。
unsafe fn toggle_visibility(hwnd: HWND) {
    let visible = IsWindowVisible(hwnd).as_bool();
    let cmd = if visible { SW_HIDE } else { SW_SHOW };
    let _ = ShowWindow(hwnd, cmd);
    crate::config::log_debug(&format!("visibility toggled: visible={}", !visible));
}
