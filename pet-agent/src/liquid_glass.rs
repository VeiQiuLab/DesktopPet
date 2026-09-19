//! LiquidGlass C++ shim 的 Rust FFI 绑定。

use std::os::raw::{c_float, c_int, c_void};

extern "C" {
    fn lg_init(hwnd: *mut c_void, width: c_int, height: c_int) -> c_int;
    fn lg_shutdown();
    fn lg_config(
        blur: c_float,
        radius: c_float,
        saturation: c_float,
        refr: c_float,
        refr_neg: c_float,
        refr_h: c_float,
        dispersion: c_float,
        darkening: c_float,
        tint_r: c_float,
        tint_g: c_float,
        tint_b: c_float,
        tint_a: c_float,
    );
    #[allow(dead_code)]
    fn lg_set_background(r: c_float, g: c_float, b: c_float);
    fn lg_capture_behind(hwnd: *mut c_void, pad: c_int);
    fn lg_render_frame(
        win_w: c_int,
        win_h: c_int,
        gx: c_float,
        gy: c_float,
        gw: c_float,
        gh: c_float,
    );
    fn lg_resize(width: c_int, height: c_int);
    fn lg_ok() -> c_int;
}

/// 玻璃参数（对应 GlassConfig 常用项）。
pub struct GlassStyle {
    /// 阴影不透明度（0 = 无）
    pub shadow: f32,
    pub blur: f32,
    pub radius: f32,
    pub saturation: f32,
    pub refraction: f32,
    pub refraction_negative: f32,
    pub refraction_height: f32,
    pub dispersion: f32,
    pub darkening: f32,
    pub tint: (f32, f32, f32, f32),
}

impl Default for GlassStyle {
    fn default() -> Self {
        // 深色 Liquid Glass：强模糊、大圆角、克制色散、轻微暗化、暗酒红 tint
        GlassStyle {
            shadow: 0.0,
            blur: 10.0,
            radius: 26.0,
            saturation: 1.25,
            refraction: 0.12,
            refraction_negative: 0.0,
            refraction_height: 0.18,
            dispersion: 0.7,
            darkening: 0.15,
            tint: (0.06, 0.06, 0.09, 0.70),
        }
    }
}

pub fn init(hwnd: *mut c_void, w: i32, h: i32) -> bool {
    unsafe { lg_init(hwnd, w, h) != 0 }
}

pub fn shutdown() {
    unsafe { lg_shutdown() }
}

pub fn config(style: &GlassStyle) {
    unsafe {
        lg_config(
            style.blur,
            style.radius,
            style.saturation,
            style.refraction,
            style.refraction_negative,
            style.refraction_height,
            style.dispersion,
            style.darkening,
            style.tint.0,
            style.tint.1,
            style.tint.2,
            style.tint.3,
        );
        let _ = style.shadow;
    }
}

#[allow(dead_code)]
pub fn set_background(r: f32, g: f32, b: f32) {
    unsafe { lg_set_background(r, g, b) }
}

/// 捕获窗口背后的桌面区域作为玻璃背景（折射真实桌面）。
pub fn capture_behind(hwnd: *mut c_void, pad: i32) {
    unsafe { lg_capture_behind(hwnd, pad) }
}

pub fn render_frame(win_w: i32, win_h: i32, gx: f32, gy: f32, gw: f32, gh: f32) {
    unsafe { lg_render_frame(win_w, win_h, gx, gy, gw, gh) }
}

#[allow(dead_code)]
pub fn resize(w: i32, h: i32) {
    unsafe { lg_resize(w, h) }
}

pub fn ok() -> bool {
    unsafe { lg_ok() != 0 }
}
