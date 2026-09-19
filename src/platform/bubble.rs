//! 桌宠文字气泡：独立的 Win32 layered window。
//!
//! - 无边框、不进任务栏、鼠标穿透、不抢焦点
//! - 极轻的浅色半透明圆角背景 + 深色文字（Microsoft YaHei UI）
//! - 自动换行、按文本计算尺寸、限制最大宽度
//! - 用 UpdateLayeredWindow + 32 位 DIB 实现逐像素 alpha

use std::sync::Once;

use windows::core::w;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::log_error;

const BUBBLE_CLASS: windows::core::PCWSTR = w!("DesktopPetBubble");
const PADDING: i32 = 12;
const CORNER_RADIUS: i32 = 14;
const MAX_WIDTH: i32 = 320;
const FONT_PT: i32 = 12;
/// 极轻的浅色半透明（Apple 风格），无黑色矩形。
const BG_A: u8 = 175;
const BG_RGB: (u8, u8, u8) = (245, 246, 248);
const FG_RGB: (u8, u8, u8) = (32, 34, 40);

static REGISTER_ONCE: Once = Once::new();

unsafe extern "system" fn bubble_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn ensure_class() {
    REGISTER_ONCE.call_once(|| unsafe {
        let instance =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(bubble_wndproc),
            hInstance: HINSTANCE(instance.0),
            lpszClassName: BUBBLE_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            log_error("bubble RegisterClassExW failed");
        }
    });
}

#[inline]
fn hobj<T: Copy>(h: T) -> HGDIOBJ {
    // 所有 GDI 句柄都是单指针包装，用 transmute 统一转 HGDIOBJ。
    unsafe { std::mem::transmute_copy(&h) }
}

pub struct BubbleWindow {
    hwnd: HWND,
    mem_dc: HDC,
    font: HFONT,
    dpi: u32,
    hbitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl BubbleWindow {
    pub unsafe fn new() -> Option<Self> {
        ensure_class();
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
        let ex_style =
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
        let hwnd = CreateWindowExW(
            ex_style,
            BUBBLE_CLASS,
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .ok()?;

        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let _ = ReleaseDC(None, screen_dc);

        let dpi = {
            let d = GetDpiForWindow(hwnd);
            if d == 0 {
                96
            } else {
                d
            }
        };
        let font = create_font(dpi);

        Some(BubbleWindow {
            hwnd,
            mem_dc,
            font,
            dpi,
            hbitmap: HBITMAP::default(),
            old_bitmap: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            width: 0,
            height: 0,
        })
    }

    #[allow(dead_code)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// 显示文本并在 (x, y) 定位。一次 UpdateLayeredWindow 完成定位 + 内容。
    pub unsafe fn show(&mut self, text: &str, x: i32, y: i32) -> bool {
        let (w, h) = match self.measure(text) {
            Some(v) => v,
            None => return false,
        };
        if !self.recreate_bitmap(w, h) {
            return false;
        }

        let bg_pm = premult(BG_RGB, BG_A);
        fill_rounded(self.bits, w, h, CORNER_RADIUS, bg_pm, BG_A);

        if let Some(mask) = self.render_text_mask(text, w, h) {
            composite_text(self.bits, w, h, &mask, FG_RGB);
        }

        if self.bits.is_null() {
            crate::config::log_error("bubble bits is null");
            return false;
        }
        let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        if !self.commit_at(w, h, x, y) {
            return false;
        }
        true
    }

    pub unsafe fn hide(&mut self) {
        let _ = ShowWindow(self.hwnd, SW_HIDE);
    }

    pub unsafe fn move_to(&self, x: i32, y: i32) {
        let _ = SetWindowPos(
            self.hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }

    pub unsafe fn measure(&self, text: &str) -> Option<(i32, i32)> {
        let dc = self.mem_dc;
        let old_font = SelectObject(dc, hobj(self.font));
        let max_text_w = scale(MAX_WIDTH - PADDING * 2, self.dpi).max(16);

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: max_text_w,
            bottom: 0,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut rc,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL,
        );
        let _ = SelectObject(dc, old_font);

        let text_w = rc.right - rc.left;
        let text_h = rc.bottom - rc.top;
        if text_w <= 0 || text_h <= 0 {
            return None;
        }
        Some((text_w + PADDING * 2, text_h + PADDING * 2))
    }

    unsafe fn render_text_mask(&self, text: &str, w: i32, h: i32) -> Option<Vec<u8>> {
        let mask_dc = CreateCompatibleDC(Some(self.mem_dc));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(Some(mask_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(mask_dc);
                return None;
            }
        };
        if bits.is_null() {
            let _ = DeleteObject(hobj(hbmp));
            let _ = DeleteDC(mask_dc);
            return None;
        }
        let old = SelectObject(mask_dc, hobj(hbmp));

        let black = CreateSolidBrush(COLORREF(0));
        let full = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        FillRect(mask_dc, &full, black);
        let _ = DeleteObject(hobj(black));

        let old_font = SelectObject(mask_dc, hobj(self.font));
        let _ = SetBkMode(mask_dc, TRANSPARENT);
        let _ = SetTextColor(mask_dc, COLORREF(0x00FF_FFFF));
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut rc = RECT {
            left: PADDING,
            top: PADDING,
            right: w - PADDING,
            bottom: h - PADDING,
        };
        DrawTextW(
            mask_dc,
            &mut wide,
            &mut rc,
            DT_WORDBREAK | DT_NOPREFIX | DT_EDITCONTROL,
        );
        let _ = SelectObject(mask_dc, old_font);

        let count = (w * h) as usize;
        let mut mask = vec![0u8; count];
        let ptr = bits as *const u8;
        for i in 0..count {
            mask[i] = *ptr.add(i * 4 + 2);
        }

        let _ = SelectObject(mask_dc, old);
        let _ = DeleteObject(hobj(hbmp));
        let _ = DeleteDC(mask_dc);
        Some(mask)
    }

    unsafe fn commit_at(&self, w: i32, h: i32, x: i32, y: i32) -> bool {
        if self.hbitmap.0.is_null() {
            return false;
        }
        let screen_dc = GetDC(None);
        let dst = POINT { x, y };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let r = UpdateLayeredWindow(
            self.hwnd,
            Some(screen_dc),
            Some(&dst),
            Some(&size),
            Some(self.mem_dc),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        let _ = ReleaseDC(None, screen_dc);
        if r.is_err() {
            let err = windows::Win32::Foundation::GetLastError();
            crate::config::log_error(&format!("UpdateLayeredWindow failed: {:?}", err));
            false
        } else {
            true
        }
    }

    unsafe fn recreate_bitmap(&mut self, w: i32, h: i32) -> bool {
        self.width = w;
        self.height = h;

        if !self.hbitmap.0.is_null() {
            let _ = SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(hobj(self.hbitmap));
            self.hbitmap = HBITMAP::default();
            self.bits = std::ptr::null_mut();
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        match CreateDIBSection(Some(self.mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(hbmp) => {
                self.hbitmap = hbmp;
                self.old_bitmap = SelectObject(self.mem_dc, hobj(hbmp));
                self.bits = bits as *mut u8;
                true
            }
            Err(_) => false,
        }
    }
}

impl Drop for BubbleWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.hbitmap.0.is_null() {
                let _ = SelectObject(self.mem_dc, self.old_bitmap);
                let _ = DeleteObject(hobj(self.hbitmap));
            }
            let _ = DeleteObject(hobj(self.font));
            let _ = DeleteDC(self.mem_dc);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

// ---------- 辅助 ----------

fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

unsafe fn create_font(dpi: u32) -> HFONT {
    let height = (FONT_PT * dpi as i32 / 72).max(12);
    CreateFontW(
        height,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Microsoft YaHei UI"),
    )
}

fn premult(rgb: (u8, u8, u8), a: u8) -> (u8, u8, u8) {
    let f = |c: u8| ((c as u16 * a as u16) / 255) as u8;
    (f(rgb.0), f(rgb.1), f(rgb.2))
}

fn fill_rounded(bits: *mut u8, w: i32, h: i32, r: i32, rgb_pm: (u8, u8, u8), a: u8) {
    if bits.is_null() {
        return;
    }
    let r = r.max(0).min(w / 2).min(h / 2);
    let buf = unsafe { std::slice::from_raw_parts_mut(bits, (w * h * 4) as usize) };
    for y in 0..h {
        for x in 0..w {
            let inside = {
                let dx = if x < r {
                    r - x
                } else if x >= w - r {
                    x - (w - r - 1)
                } else {
                    0
                };
                let dy = if y < r {
                    r - y
                } else if y >= h - r {
                    y - (h - r - 1)
                } else {
                    0
                };
                if dx > 0 && dy > 0 {
                    dx * dx + dy * dy <= r * r
                } else {
                    true
                }
            };
            if !inside {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            buf[i] = rgb_pm.2;
            buf[i + 1] = rgb_pm.1;
            buf[i + 2] = rgb_pm.0;
            buf[i + 3] = a;
        }
    }
}

fn composite_text(bits: *mut u8, w: i32, h: i32, mask: &[u8], fg: (u8, u8, u8)) {
    if bits.is_null() {
        return;
    }
    let count = (w * h) as usize;
    let buf = unsafe { std::slice::from_raw_parts_mut(bits, count * 4) };
    let n = count.min(mask.len());
    for i in 0..n {
        let t = mask[i] as u32;
        if t == 0 {
            continue;
        }
        let idx = i * 4;
        let bg_b = buf[idx] as u32;
        let bg_g = buf[idx + 1] as u32;
        let bg_r = buf[idx + 2] as u32;
        let bg_a = buf[idx + 3] as u32;
        let inv = 255 - t;
        let out_b = (fg.2 as u32 * t + bg_b * inv) / 255;
        let out_g = (fg.1 as u32 * t + bg_g * inv) / 255;
        let out_r = (fg.0 as u32 * t + bg_r * inv) / 255;
        let out_a = (255 * t + bg_a * inv) / 255;
        buf[idx] = out_b.min(255) as u8;
        buf[idx + 1] = out_g.min(255) as u8;
        buf[idx + 2] = out_r.min(255) as u8;
        buf[idx + 3] = out_a.min(255) as u8;
    }
}
