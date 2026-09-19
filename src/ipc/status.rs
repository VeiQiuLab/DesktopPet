//! 桌宠状态快照（供 IPC 只读查询）。
//!
//! 主线程每帧更新；IPC worker 读取。全部为原子/短锁，避免跨线程访问 HWND。

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

pub struct SharedStatus {
    visible: AtomicBool,
    /// 窗口矩形 [left, top, right, bottom]
    rect: [AtomicI32; 4],
    /// 模型可见几何包围盒（窗口客户区内像素，左上原点）；valid=false 表示无效
    vis_rect: [AtomicI32; 4],
    vis_valid: AtomicBool,
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
            vis_rect: [
                AtomicI32::new(0),
                AtomicI32::new(0),
                AtomicI32::new(0),
                AtomicI32::new(0),
            ],
            vis_valid: AtomicBool::new(false),
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

    /// 设置模型可见几何包围盒（窗口客户区内像素）。传 None 清除。
    pub fn set_visible_bounds(&self, b: Option<(f32, f32, f32, f32)>) {
        match b {
            Some((l, t, r, bot)) => {
                self.vis_rect[0].store(l as i32, Ordering::Relaxed);
                self.vis_rect[1].store(t as i32, Ordering::Relaxed);
                self.vis_rect[2].store(r as i32, Ordering::Relaxed);
                self.vis_rect[3].store(bot as i32, Ordering::Relaxed);
                self.vis_valid.store(true, Ordering::Relaxed);
            }
            None => self.vis_valid.store(false, Ordering::Relaxed),
        }
    }

    pub fn set_active_character(&self, id: &str) {
        if let Ok(mut g) = self.active_character.lock() {
            if g.as_str() != id {
                *g = id.to_string();
            }
        }
    }

    pub fn snapshot(&self) -> pet_protocol::StatusSnapshot {
        let l = self.rect[0].load(Ordering::Relaxed);
        let t = self.rect[1].load(Ordering::Relaxed);
        let visible_rect = if self.vis_valid.load(Ordering::Relaxed) {
            // 窗口客户区坐标 → 屏幕坐标（窗口为 WS_POPUP，客户区=窗口区）
            Some([
                l + self.vis_rect[0].load(Ordering::Relaxed),
                t + self.vis_rect[1].load(Ordering::Relaxed),
                l + self.vis_rect[2].load(Ordering::Relaxed),
                t + self.vis_rect[3].load(Ordering::Relaxed),
            ])
        } else {
            None
        };
        pet_protocol::StatusSnapshot {
            online: true,
            visible: self.visible.load(Ordering::Relaxed),
            window_rect: [
                l,
                t,
                self.rect[2].load(Ordering::Relaxed),
                self.rect[3].load(Ordering::Relaxed),
            ],
            active_character: self
                .active_character
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            visible_rect,
        }
    }
}

impl Default for SharedStatus {
    fn default() -> Self {
        Self::new()
    }
}
