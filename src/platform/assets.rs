//! Windows 图标资源加载。
//!
//! 应用图标以 ID 1 / 2 编入 exe 资源（见 assets/desktop-pet.rc）；
//! 运行时通过 LoadImageW 从**当前进程模块**取图标，用于窗口类与托盘。
//!
//! 注意：不能用 LoadIconW(None, ...)，它不一定指向当前 exe；必须显式
//! 传 GetModuleHandleW(None) 拿到的 HINSTANCE。

use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    LoadImageW, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED,
};

/// 应用主图标资源 ID（与 .rc 中一致）。
const APP_ICON_RESOURCE_ID: u16 = 1;
/// 托盘图标资源 ID。
const TRAY_ICON_RESOURCE_ID: u16 = 2;

/// 从当前进程模块按 ID 加载图标（多尺寸 SHARED 缓存）。
/// 失败时回退到系统默认应用图标。
unsafe fn load_module_icon(id: u16) -> HICON {
    // 显式取当前 exe 模块句柄（不能用 HINSTANCE(NULL) 走 LoadIconW 的模糊解析）
    let hmod = match GetModuleHandleW(None) {
        Ok(h) => h,
        Err(e) => {
            crate::config::log_error(&format!("GetModuleHandleW failed: {e:?}"));
            return LoadImageW(
                None,
                PCWSTR(IDI_APPLICATION.0 as *const u16),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            )
            .map(|h| HICON(h.0))
            .unwrap_or_default();
        }
    };

    // MAKEINTRESOURCE(id)：高位为 0 时按资源 ID 解析
    let res = PCWSTR(id as usize as *const u16);
    match LoadImageW(
        Some(hmod.into()),
        res,
        IMAGE_ICON,
        0,
        0,
        LR_DEFAULTSIZE | LR_SHARED,
    ) {
        Ok(h) => HICON(h.0),
        Err(e) => {
            crate::config::log_error(&format!("LoadImageW(id={id}) failed: {e:?}"));
            // 回退
            LoadImageW(
                None,
                PCWSTR(IDI_APPLICATION.0 as *const u16),
                IMAGE_ICON,
                0,
                0,
                LR_DEFAULTSIZE | LR_SHARED,
            )
            .map(|h| HICON(h.0))
            .unwrap_or_default()
        }
    }
}

/// 加载应用主图标（EXE 资源 ID 1）。
pub unsafe fn load_app_icon() -> HICON {
    load_module_icon(APP_ICON_RESOURCE_ID)
}

/// 加载托盘图标（ID 2，头部特写版）。失败时回退到主图标。
pub unsafe fn load_tray_icon() -> HICON {
    let h = load_module_icon(TRAY_ICON_RESOURCE_ID);
    if h.0.is_null() {
        load_app_icon()
    } else {
        h
    }
}
