//! 桌宠状态快照（供 IPC 只读查询）。
//!
//! 主线程每帧更新；IPC worker 读取。全部为原子/短锁，避免跨线程访问 HWND。

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

pub struct SharedStatus {
    visible: AtomicBool,
    /// 窗口矩形 [left, top, right, bottom]
    rect: [AtomicI32; 4],
    active_character: Mutex<String>,
}

impl SharedStatus {
    pub fn new() -> Self {
        SharedStatus {
            visible: AtomicBool::new(true),
            rect: [
                AtomicI32::new(0),
                AtomicI32::new(0),
                AtomicI32::new(0),
                AtomicI32::new(0),
            ],
            active_character: Mutex::new(String::new()),
        }
    }

    pub fn set_visible(&self, v: bool) {
        self.visible.store(v, Ordering::Relaxed);
    }

    pub fn set_rect(&self, l: i32, t: i32, r: i32, b: i32) {
        self.rect[0].store(l, Ordering::Relaxed);
        self.rect[1].store(t, Ordering::Relaxed);
        self.rect[2].store(r, Ordering::Relaxed);
        self.rect[3].store(b, Ordering::Relaxed);
    }

    pub fn set_active_character(&self, id: &str) {
        if let Ok(mut g) = self.active_character.lock() {
            if g.as_str() != id {
                *g = id.to_string();
            }
        }
    }

    pub fn snapshot(&self) -> pet_protocol::StatusSnapshot {
        pet_protocol::StatusSnapshot {
            online: true,
            visible: self.visible.load(Ordering::Relaxed),
            window_rect: [
                self.rect[0].load(Ordering::Relaxed),
                self.rect[1].load(Ordering::Relaxed),
                self.rect[2].load(Ordering::Relaxed),
                self.rect[3].load(Ordering::Relaxed),
            ],
            active_character: self
                .active_character
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
        }
    }
}

impl Default for SharedStatus {
    fn default() -> Self {
        Self::new()
    }
}
