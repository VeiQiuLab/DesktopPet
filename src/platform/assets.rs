//! Windows 图标资源加载。
//!
//! 应用图标以 ID 1 编入 exe 资源（见 assets/desktop-pet.rc）；
//! 运行时通过 LoadIconW 取大/小图标，用于窗口类与任务栏。

use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, HICON, IDI_APPLICATION};

/// 应用主图标资源 ID（与 .rc 中一致）。
const APP_ICON_RESOURCE_ID: usize = 1;
/// 托盘图标资源 ID。
const TRAY_ICON_RESOURCE_ID: usize = 2;

/// 从本进程资源加载指定 ID 的图标；失败时回退到系统默认图标。
unsafe fn load_resource_icon(id: usize) -> HICON {
    match LoadIconW(None, PCWSTR(id as *const u16)) {
        Ok(h) => h,
        Err(_) => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
    }
}

/// 加载应用主图标（EXE 资源 ID 1）。失败时回退到系统默认图标。
pub unsafe fn load_app_icon() -> HICON {
    load_resource_icon(APP_ICON_RESOURCE_ID)
}

/// 加载托盘图标（ID 2，头部特写版）。失败时回退到主图标。
pub unsafe fn load_tray_icon() -> HICON {
    let h = load_resource_icon(TRAY_ICON_RESOURCE_ID);
    if h.0.is_null() {
        load_app_icon()
    } else {
        h
    }
}
