//! 统一深色主题：背景/文字色、字体、WM_CTLCOLOR 处理。
//!
//! 原生控件的深色化通过 WM_CTLCOLORSTATIC/EDIT/LISTBOX 返回深色画刷 + 浅色文字实现；
//! 按钮保留系统绘制（避免 owner-draw 风险）。

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, GetStockObject, SetBkColor, SetBkMode, SetTextColor,
    DEFAULT_GUI_FONT, HBRUSH, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 主题色（RGB）。
pub const BG: (u8, u8, u8) = (30, 32, 38);
pub const PANEL: (u8, u8, u8) = (40, 43, 50);
pub const FG: (u8, u8, u8) = (230, 232, 236);
#[allow(dead_code)]
pub const ACCENT: (u8, u8, u8) = (90, 140, 240);

fn rgb(c: (u8, u8, u8)) -> COLORREF {
    COLORREF((c.0 as u32) | ((c.1 as u32) << 8) | ((c.2 as u32) << 16))
}

static mut BG_BRUSH: isize = 0;

/// 惰性创建背景画刷。
pub unsafe fn bg_brush() -> HBRUSH {
    if BG_BRUSH == 0 {
        BG_BRUSH = CreateSolidBrush(rgb(BG)).0 as isize;
    }
    HBRUSH(BG_BRUSH as *mut _)
}

/// 中文字体（Microsoft YaHei UI），dpi 缩放。
pub unsafe fn create_font(dpi: u32, pt: i32) -> HGDIOBJ {
    use windows::Win32::Graphics::Gdi::{
        CreateFontW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FW_NORMAL,
        OUT_DEFAULT_PRECIS,
    };
    let h = (pt * dpi as i32 / 72).max(12);
    let _ = (OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS);
    let f = CreateFontW(
        h,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        windows::Win32::Graphics::Gdi::OUT_DEFAULT_PRECIS,
        windows::Win32::Graphics::Gdi::CLIP_DEFAULT_PRECIS,
        windows::Win32::Graphics::Gdi::CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Microsoft YaHei UI"),
    );
    HGDIOBJ(f.0)
}

/// 处理 WM_CTLCOLOR*：返回深色画刷并设置文字色。
pub unsafe fn on_ctlcolor(_hwnd: HWND, msg: u32, wparam: WPARAM) -> LRESULT {
    let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
    match msg {
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX => {
            let _ = SetTextColor(hdc, rgb(FG));
            let _ = SetBkMode(hdc, TRANSPARENT);
            LRESULT(bg_brush().0 as isize)
        }
        WM_CTLCOLOREDIT => {
            let _ = SetTextColor(hdc, rgb(FG));
            let _ = SetBkColor(hdc, rgb(PANEL));
            LRESULT(bg_brush().0 as isize)
        }
        _ => LRESULT(0),
    }
}

/// 给指定控件设置主题字体。
pub unsafe fn apply_font(hwnd_ctrl: HWND, font: HGDIOBJ) {
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    let _ = SendMessageW(
        hwnd_ctrl,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
}

/// 默认系统字体（备选）。
#[allow(dead_code)]
pub unsafe fn default_font() -> HGDIOBJ {
    GetStockObject(DEFAULT_GUI_FONT)
}

/// 释放（进程退出时可忽略；此处提供对称清理）。
#[allow(dead_code)]
pub unsafe fn cleanup() {
    if BG_BRUSH != 0 {
        let _ = DeleteObject(HGDIOBJ(BG_BRUSH as *mut _));
        BG_BRUSH = 0;
    }
}

/// 未使用的占位，避免 dead_code。
pub fn _placeholder() {
    let _ = PCWSTR::null();
}
