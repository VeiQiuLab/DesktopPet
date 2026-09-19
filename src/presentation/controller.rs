//! PresentationController：管理气泡生命周期、表达队列、Idle 台词调度。
//!
//! 只依赖 platform::bubble（Win32）与 behavior::PetAction；不直接接触 Cubism。

use std::collections::VecDeque;

use crate::behavior::PetAction;
use crate::character::CharacterPackage;
use crate::config::log_error;
use crate::platform::bubble::BubbleWindow;
use crate::platform::window::pet_rect;

use super::expression::{PetExpression, Priority};

/// 队列硬上限。
const MAX_QUEUE: usize = 8;
/// 展示时长上下限（秒）。
const MIN_DURATION: f32 = 2.0;
const MAX_DURATION: f32 = 12.0;

struct Queued {
    text: String,
    priority: Priority,
    duration: Option<f32>,
}

/// 气泡位置矩形（屏幕坐标）。
#[derive(Clone, Copy)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub struct PresentationController {
    bubble: Option<BubbleWindow>,
    queue: VecDeque<Queued>,
    remaining: f32,
    current_text: String,
    showing: bool,

    idle_next_in: f32,
    suppress_until: f32,

    greeting: Option<String>,
    idle_pool: Vec<String>,
    click_pool: Vec<String>,
    dbl_pool: Vec<String>,
    click_prob: f32,
    dbl_prob: f32,
    idle_speech_min: f32,
    idle_speech_max: f32,
    idle_speech_suppress: f32,

    rng: Rng,
    pet_hwnd: windows::Win32::Foundation::HWND,
}

impl PresentationController {
    pub fn new(pkg: &CharacterPackage) -> Self {
        let sp = pkg.speech().clone();
        let mut rng = Rng::new();
        let idle_next_in = rng.range(sp.idle_speech_interval_min, sp.idle_speech_interval_max);
        PresentationController {
            bubble: unsafe { BubbleWindow::new() },
            queue: VecDeque::new(),
            remaining: 0.0,
            current_text: String::new(),
            showing: false,
            idle_next_in,
            suppress_until: 0.0,
            greeting: sp.greeting.clone(),
            idle_pool: sp.idle.clone(),
            click_pool: sp.click.clone(),
            dbl_pool: sp.double_click.clone(),
            click_prob: sp.click_speech_probability,
            dbl_prob: sp.double_click_speech_probability,
            idle_speech_min: sp.idle_speech_interval_min,
            idle_speech_max: sp.idle_speech_interval_max,
            idle_speech_suppress: sp.idle_speech_suppress_after_interaction,
            rng,
            pet_hwnd: windows::Win32::Foundation::HWND::default(),
        }
    }

    /// 启动问候（若有配置）。
    pub fn greet(&mut self) {
        if let Some(g) = self.greeting.clone() {
            if !g.is_empty() {
                self.enqueue(g, Priority::System, None);
            }
        }
    }

    /// 切换角色：更新台词配置，清空旧状态。
    pub fn set_character(&mut self, pkg: &CharacterPackage) {
        let sp = pkg.speech();
        self.greeting = sp.greeting.clone();
        self.idle_pool = sp.idle.clone();
        self.click_pool = sp.click.clone();
        self.dbl_pool = sp.double_click.clone();
        self.click_prob = sp.click_speech_probability;
        self.dbl_prob = sp.double_click_speech_probability;
        self.idle_speech_min = sp.idle_speech_interval_min;
        self.idle_speech_max = sp.idle_speech_interval_max;
        self.idle_speech_suppress = sp.idle_speech_suppress_after_interaction;
        self.queue.clear();
        self.hide();
        self.idle_next_in = self.rng.range(self.idle_speech_min, self.idle_speech_max);
    }

    /// 提交一个表达。返回需要触发的动作（若有）。
    pub fn present(&mut self, expr: PetExpression) -> Option<PetAction> {
        let priority = expr.priority();
        let motion = expr.motion();

        if let Some(text) = expr.text() {
            self.enqueue(text.to_string(), priority, expr.duration());
        }
        if priority >= Priority::User {
            self.suppress_until = self.idle_speech_suppress;
        }
        motion
    }

    fn enqueue(&mut self, text: String, priority: Priority, duration: Option<f32>) {
        if text.trim().is_empty() {
            return;
        }
        if self
            .queue
            .iter()
            .any(|q| q.text == text && q.priority == priority)
        {
            return;
        }
        if priority > Priority::Idle {
            self.queue.retain(|q| q.text != text);
        }
        if self.queue.len() >= MAX_QUEUE {
            if let Some(pos) = self
                .queue
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| q.priority)
                .map(|(i, _)| i)
            {
                self.queue.remove(pos);
            }
        }
        self.queue.push_back(Queued {
            text,
            priority,
            duration,
        });
    }

    /// 每帧调用。pet_hwnd 用于气泡定位。
    pub fn tick_at(
        &mut self,
        dt: f32,
        visible: bool,
        interaction_blocked: bool,
        pet_hwnd: windows::Win32::Foundation::HWND,
    ) -> Option<PetAction> {
        self.pet_hwnd = pet_hwnd;
        self.tick(dt, visible, interaction_blocked)
    }

    /// 每帧调用。visible=桌宠是否可见；interaction_blocked=拖拽/菜单打开。
    /// 返回本帧应触发的动作（若有）。
    pub fn tick(&mut self, dt: f32, visible: bool, interaction_blocked: bool) -> Option<PetAction> {
        if !visible {
            if self.showing {
                self.hide();
            }
            return None;
        }

        if self.suppress_until > 0.0 {
            self.suppress_until = (self.suppress_until - dt).max(0.0);
        }

        if self.showing {
            self.remaining -= dt;
            if self.remaining <= 0.0 {
                self.hide();
            }
        }

        if !self.showing {
            if let Some(q) = self.queue.pop_front() {
                self.show_text(&q.text, q.duration);
            }
        }

        if !self.showing
            && !interaction_blocked
            && self.suppress_until <= 0.0
            && !self.idle_pool.is_empty()
        {
            self.idle_next_in -= dt;
            if self.idle_next_in <= 0.0 {
                self.idle_next_in = self.rng.range(self.idle_speech_min, self.idle_speech_max);
                let r = self.rng.next_f32();
                if let Some(s) = pick(&self.idle_pool, r) {
                    let s = s.to_string();
                    self.show_text(&s, None);
                }
            }
        }

        None
    }

    /// 单击反馈台词（按概率）。
    pub fn on_click(&mut self) {
        self.suppress_until = self.idle_speech_suppress;
        if !self.click_pool.is_empty() && self.rng.next_f32() < self.click_prob {
            let r = self.rng.next_f32();
            if let Some(s) = pick(&self.click_pool, r) {
                let s = s.to_string();
                self.enqueue(s, Priority::User, None);
            }
        }
    }

    /// 双击反馈台词（按概率）。
    pub fn on_double_click(&mut self) {
        self.suppress_until = self.idle_speech_suppress;
        if !self.dbl_pool.is_empty() && self.rng.next_f32() < self.dbl_prob {
            let r = self.rng.next_f32();
            if let Some(s) = pick(&self.dbl_pool, r) {
                let s = s.to_string();
                self.enqueue(s, Priority::User, None);
            }
        }
    }

    /// 用户交互（拖拽等）：抑制 idle 台词。
    pub fn on_interaction(&mut self) {
        self.suppress_until = self.idle_speech_suppress;
    }

    /// 强制隐藏气泡。
    pub fn hide(&mut self) {
        if let Some(b) = self.bubble.as_mut() {
            unsafe { b.hide() };
        }
        self.showing = false;
        self.current_text.clear();
        self.remaining = 0.0;
    }

    #[allow(dead_code)]
    pub fn is_showing(&self) -> bool {
        self.showing
    }

    fn show_text(&mut self, text: &str, duration: Option<f32>) {
        let b = match self.bubble.as_ref() {
            Some(b) => b,
            None => return,
        };
        // 先测量尺寸以计算位置
        let (bw, bh) = unsafe { b.measure(text) }.unwrap_or((200, 40));
        let (x, y) = unsafe { compute_position(self.pet_hwnd, bw, bh) }.unwrap_or((10, 10));
        let b = self.bubble.as_mut().unwrap();
        if unsafe { !b.show(text, x, y) } {
            log_error("bubble show failed");
            return;
        }
        self.current_text = text.to_string();
        self.showing = true;
        self.remaining = duration.unwrap_or_else(|| estimate_duration(text));
    }

    /// 根据桌宠窗口位置重新定位气泡。
    pub fn reposition(&mut self, pet_hwnd: windows::Win32::Foundation::HWND) {
        let b = match self.bubble.as_ref() {
            Some(b) => b,
            None => return,
        };
        if !self.showing {
            return;
        }
        let (pw, ph) = b.size();
        if let Some((x, y)) = unsafe { compute_position(pet_hwnd, pw, ph) } {
            unsafe { b.move_to(x, y) };
        }
    }
}

fn estimate_duration(text: &str) -> f32 {
    let mut units = 0.0f32;
    for ch in text.chars() {
        if ch.is_ascii() {
            units += 0.5;
        } else {
            units += 1.0;
        }
    }
    let d = 1.5 + units / 5.0;
    d.clamp(MIN_DURATION, MAX_DURATION)
}

unsafe fn compute_position(
    pet_hwnd: windows::Win32::Foundation::HWND,
    bw: i32,
    bh: i32,
) -> Option<(i32, i32)> {
    let rect = pet_rect(pet_hwnd)?;
    let pet_w = rect.right - rect.left;
    let pet_h = rect.bottom - rect.top;

    let mut x = rect.left + pet_w / 4;
    let mut y = rect.top - bh - 8;

    let wa = crate::platform::window::monitor_work_area(pet_hwnd).unwrap_or(Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    });

    if y < wa.top {
        y = rect.top + pet_h + 8;
    }
    if x + bw > wa.right {
        x = wa.right - bw - 4;
    }
    if x < wa.left {
        x = wa.left + 4;
    }
    if y + bh > wa.bottom {
        y = wa.bottom - bh - 4;
    }
    if y < wa.top {
        y = wa.top + 4;
    }
    Some((x, y))
}

fn pick<'a>(pool: &'a [String], r: f32) -> Option<&'a str> {
    if pool.is_empty() {
        return None;
    }
    let idx = ((r.clamp(0.0, 0.9999)) * pool.len() as f32) as usize;
    pool.get(idx).map(|s| s.as_str())
}

struct Rng {
    state: u64,
}

impl Rng {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Rng { state: seed | 1 }
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, min: f32, max: f32) -> f32 {
        if max <= min {
            return min.max(0.0);
        }
        min + self.next_f32() * (max - min)
    }
}
