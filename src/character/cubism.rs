//! Cubism C++ shim 的 FFI 绑定与安全封装。
//!
//! C++ 侧负责 Live2D 模型解析、物理、动作与 D3D11 绘制；
//! Rust 侧仅通过本模块传递设备指针与每帧指令。

use std::ffi::CString;
use std::os::raw::{c_char, c_float, c_int, c_void};

use crate::config::log_line;

extern "C" {
    fn cubism_shim_model_hit_test(
        handle: *mut c_void,
        x: c_float,
        y: c_float,
        w: c_float,
        h: c_float,
    ) -> c_int;
    fn cubism_shim_version() -> c_int;
    fn cubism_shim_startup() -> c_int;
    fn cubism_shim_shutdown();
    fn cubism_shim_set_device(device: *mut c_void, context: *mut c_void);
    fn cubism_shim_model_load(
        model3_path: *const c_char,
        width: c_int,
        height: c_int,
    ) -> *mut c_void;
    fn cubism_shim_model_free(handle: *mut c_void);
    fn cubism_shim_model_update(handle: *mut c_void, dt: c_float);
    fn cubism_shim_model_draw(handle: *mut c_void, width: c_float, height: c_float);
    fn cubism_shim_model_start_motion(
        handle: *mut c_void,
        group: *const c_char,
        no: c_int,
        priority: c_int,
    ) -> c_int;
}

/// 原始句柄直接命中测试（供窗口 wndproc 使用，避免借用 CubismModel）。
///
/// # Safety
/// handle 必须是 CubismModel::load 返回且仍存活的句柄。
pub unsafe fn hit_test_raw(handle: *mut c_void, x: f32, y: f32, w: f32, h: f32) -> bool {
    if handle.is_null() {
        return false;
    }
    cubism_shim_model_hit_test(handle, x, y, w, h) != 0
}

/// Live2D 模型句柄（不透明指针）。
pub struct CubismModel {
    handle: *mut c_void,
}

impl CubismModel {
    /// 启动 Cubism Framework（全局，只需一次）。
    pub fn startup() -> bool {
        unsafe {
            let v = cubism_shim_version();
            log_line(&format!("cubism shim version: {v}"));
            cubism_shim_startup() == 0
        }
    }

    pub fn shutdown() {
        unsafe { cubism_shim_shutdown() }
    }

    /// 传入 D3D11 设备与上下文指针（须在加载模型前调用）。
    pub fn set_device(device: *mut c_void, context: *mut c_void) {
        unsafe { cubism_shim_set_device(device, context) }
    }

    /// 加载模型。返回 None 表示失败。
    pub fn load(model3_path: &str, width: i32, height: i32) -> Option<Self> {
        let path = CString::new(model3_path).ok()?;
        let handle = unsafe { cubism_shim_model_load(path.as_ptr(), width, height) };
        if handle.is_null() {
            log_line(&format!("failed to load model: {model3_path}"));
            None
        } else {
            log_line(&format!("model loaded: {model3_path}"));
            Some(CubismModel { handle })
        }
    }

    pub fn update(&self, dt: f32) {
        unsafe { cubism_shim_model_update(self.handle, dt) }
    }

    pub fn draw(&self, width: f32, height: f32) {
        unsafe { cubism_shim_model_draw(self.handle, width, height) }
    }

    /// 原始句柄（供窗口层 wndproc 做命中测试，不介入生命周期管理）。
    pub fn raw_handle(&self) -> *mut c_void {
        self.handle
    }

    /// 播放动作。返回 true 表示成功。优先级：1=Idle, 2=Normal, 3=Force。
    pub fn start_motion(&self, group: &str, no: i32, priority: i32) -> bool {
        let g = match CString::new(group) {
            Ok(g) => g,
            Err(_) => return false,
        };
        unsafe { cubism_shim_model_start_motion(self.handle, g.as_ptr(), no, priority) == 0 }
    }
}

impl Drop for CubismModel {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { cubism_shim_model_free(self.handle) }
            self.handle = std::ptr::null_mut();
        }
    }
}
