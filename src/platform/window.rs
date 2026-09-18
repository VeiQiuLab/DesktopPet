//! Win32 透明置顶窗口 + 消息处理 + 鼠标拖拽 + 事件采集。
//!
//! 关键实现：
//! - WS_EX_NOREDIRECTIONBITMAP：窗口不占用 GDI 重定向缓冲，配合 DirectComposition 合成。
//! - WM_NCHITTEST：通过 Cubism 几何命中判断，模型区域返回 HTCLIENT，透明区域返回 HTTRANSPARENT（穿透）。
//! - 拖拽仅可在命中模型时启动；拖拽开始/结束通过 channel 上报事件。
//! - 本层不决定行为，仅采集输入事件（PetEvent）。

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ScreenToClient, UpdateWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::behavior::PetEvent;
use crate::character::cubism;
use crate::config::{log_line, Config};

/// 拖拽位移阈值（像素）：超过该值视为拖拽，否则视为点击。
const DRAG_THRESHOLD: i32 = 5;
/// 双击判定窗口（毫秒）。
const DOUBLE_CLICK_MS: u128 = 400;

/// 窗口创建参数（来自配置）。
pub struct WindowParams {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 运行态窗口状态。
pub struct WindowState {
    #[allow(dead_code)]
    pub hwnd: HWND,
    pub dragging: bool,
    pub drag_moved: bool,
    pub drag_origin: POINT,
    pub window_origin: POINT,
    /// 模型句柄（app 加载模型后由 set_model 挂上）。
    pub model_handle: *mut c_void,
    /// 窗口客户区尺寸（像素），用于命中测试。
    pub client_size: (i32, i32),
    /// 事件发送端（主循环在另一端接收）。
    pub events: Sender<PetEvent>,
    /// 上一次点击时间，用于双击判定。
    pub last_click: Option<Instant>,
    /// 鼠标当前是否悬停在角色上。
    pub hovering: bool,
    pub config: Config,
}

/// 注册窗口类并创建窗口。
pub unsafe fn create_window(
    params: &WindowParams,
    config: Config,
    events: Sender<PetEvent>,
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
    });
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = UpdateWindow(hwnd);
    log_line("window created");
    Ok(hwnd)
}

/// 将已加载的模型句柄挂到窗口，用于命中测试。
///
/// # Safety
/// hwnd 必须由 create_window 创建；handle 必须由 CubismModel::load 返回且存活。
pub unsafe fn set_model(hwnd: HWND, handle: *mut c_void) {
    let st = state_ptr(hwnd);
    if !st.is_null() {
        (*st).model_handle = handle;
    }
}

unsafe fn state_ptr(hwnd: HWND) -> *mut WindowState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState
}

/// 判断给定屏幕坐标是否命中模型（使用 C++ shim 的几何命中）。
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
                let _ = (*st).events.send(PetEvent::RightClick);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x1B {
                let _ = DestroyWindow(hwnd);
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
                drop(Box::from_raw(st));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
