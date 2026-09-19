//! 嘴型同步控制器（DesktopPet 侧）。
//!
//! 只认识「归一化 mouth amplitude」；不加载音频、不播声音、不解析 WAV、不知道 SAPI。
//! 接收完整 envelope（含 sample_hz / start_delay），每帧按 elapsed 查找 amplitude 并平滑。

const ATTACK: f32 = 0.7;
const RELEASE: f32 = 0.25;

pub struct LipSyncController {
    sample_hz: f32,
    samples: Vec<f32>,
    /// 开始时刻（Instant）。
    start: Option<std::time::Instant>,
    start_delay_ms: u32,
    current: f32,
    active: bool,
}

impl LipSyncController {
    pub fn new() -> Self {
        LipSyncController {
            sample_hz: 30.0,
            samples: Vec::new(),
            start: None,
            start_delay_ms: 0,
            current: 0.0,
            active: false,
        }
    }

    /// 设置一段完整 envelope 并立即开始。
    pub fn set_envelope(&mut self, sample_hz: f32, samples: Vec<f32>, start_delay_ms: u32) {
        if samples.is_empty() {
            self.clear();
            return;
        }
        self.sample_hz = sample_hz.clamp(1.0, 120.0);
        self.samples = samples;
        self.start_delay_ms = start_delay_ms;
        self.start = Some(std::time::Instant::now());
        self.active = true;
    }

    /// 停止并立即归零。
    pub fn clear(&mut self) {
        self.samples.clear();
        self.start = None;
        self.active = false;
        self.current = 0.0;
    }

    /// 每帧调用，返回当前 mouth_open（0..1）。
    pub fn tick(&mut self, dt: f32) -> f32 {
        if !self.active {
            // 平滑归零
            self.current = (self.current - RELEASE * dt * 8.0).max(0.0);
            return self.current;
        }
        let start = match self.start {
            Some(t) => t,
            None => return 0.0,
        };
        let elapsed = start.elapsed().as_secs_f32() * 1000.0 - self.start_delay_ms as f32;
        if elapsed < 0.0 {
            return self.current;
        }
        let idx = ((elapsed / 1000.0) * self.sample_hz) as usize;
        let target = if idx < self.samples.len() {
            self.samples[idx]
        } else {
            // 播放结束：归零并失活
            self.active = false;
            self.samples.clear();
            0.0
        };
        // attack / release 平滑
        let factor = if target > self.current {
            ATTACK
        } else {
            RELEASE
        };
        // 与帧率无关的指数平滑
        let a = 1.0 - (-factor * dt * 30.0).exp();
        self.current += (target - self.current) * a.clamp(0.0, 1.0);
        self.current.clamp(0.0, 1.0)
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for LipSyncController {
    fn default() -> Self {
        Self::new()
    }
}
