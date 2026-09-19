//! Behavior 层：事件 → 语义动作调度。
//!
//! 从「单 pending + 优先级覆盖」升级为轻量调度器：
//! - pending queue（用户动作）
//! - priority / interruptible
//! - cooldown / min interval
//! - 动作完成检测（基于 shim 的 is_busy + 超时兜底）
//! - 空闲随机行为（Blink / Nod / Shake）
//!
//! 调度完全基于主循环的 dt，帧率无关，无后台线程、无阻塞。

use std::collections::VecDeque;

use crate::character::package::IdleBehavior;

/// 由窗口层采集的语义事件。
#[derive(Debug, Clone)]
pub enum PetEvent {
    PointerEnter,
    PointerLeave,
    LeftClick,
    DoubleClick,
    DragStart,
    DragEnd,
    RightClick,
    MenuNod,
    MenuShake,
    MenuReset,
    MenuQuit,
    /// 托盘：测试气泡（由 app 处理，不经 Behavior）。
    MenuTestBubble,
    /// 托盘：切换到指定角色 id（由 app 处理，不经 Behavior）。
    TraySwitchCharacter(String),
}

/// 语义动作（与底层 motion group 解耦）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PetAction {
    Idle,
    Blink,
    Nod,
    Shake,
    ResetPosition,
    Quit,
}

impl PetAction {
    pub fn name(&self) -> &'static str {
        match self {
            PetAction::Idle => "Idle",
            PetAction::Blink => "Blink",
            PetAction::Nod => "Nod",
            PetAction::Shake => "Shake",
            PetAction::ResetPosition => "ResetPosition",
            PetAction::Quit => "Quit",
        }
    }

    /// 是否为控制类动作（重置位置 / 退出），可绕过交互阻塞。
    fn is_control(&self) -> bool {
        matches!(self, PetAction::ResetPosition | PetAction::Quit)
    }
}

/// 每帧调度上下文。
pub struct TickContext {
    /// shim 报告是否有非 Idle 动作正在播放。
    pub is_motion_busy: bool,
    /// 是否交互阻塞（拖拽 / 菜单打开）—— 阻塞时只允许控制类动作。
    pub interaction_blocked: bool,
}

struct RunningAction {
    #[allow(dead_code)]
    action: PetAction,
    #[allow(dead_code)]
    priority: i32,
    elapsed: f32,
    /// 是否曾经观察到 motion busy（用于判断动作真正启动过）。
    seen_busy: bool,
}

/// 轻量 RNG（xorshift64）。避免引入 rand 依赖。
struct Rng {
    state: u64,
}

impl Rng {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Rng {
            state: seed | 1, // 避免全零
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// [0, 1)
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

/// 用户动作优先级常量。
const PRI_NORMAL: i32 = 2;
const PRI_FORCE: i32 = 3;
const PRI_CONTROL: i32 = 10;

/// 动作启动后，等待 is_busy 变 true 的宽限时间（秒）。
const START_GRACE: f32 = 2.0;

pub struct BehaviorController {
    queue: VecDeque<(PetAction, i32)>,
    current: Option<RunningAction>,
    cooldown: f32,
    next_blink_in: f32,
    next_random_in: f32,
    rng: Rng,
    idle: IdleBehavior,
}

impl BehaviorController {
    pub fn new(idle: IdleBehavior) -> Self {
        let mut rng = Rng::new();
        let next_blink_in = rng.range(idle.blink_interval_min, idle.blink_interval_max);
        let next_random_in = idle.min_interval;
        BehaviorController {
            queue: VecDeque::new(),
            current: None,
            cooldown: 0.0,
            next_blink_in,
            next_random_in,
            rng,
            idle,
        }
    }

    /// 使用新的角色 idle 参数重建（切换角色时调用）。
    pub fn set_idle(&mut self, idle: IdleBehavior) {
        self.idle = idle;
        self.next_blink_in = self
            .rng
            .range(self.idle.blink_interval_min, self.idle.blink_interval_max);
        self.next_random_in = self.idle.min_interval;
    }

    /// 处理一个输入事件。
    pub fn handle(&mut self, ev: PetEvent) {
        match ev {
            PetEvent::LeftClick => self.enqueue(PetAction::Nod, PRI_NORMAL),
            PetEvent::DoubleClick => self.enqueue(PetAction::Shake, PRI_FORCE),
            PetEvent::MenuNod => self.enqueue(PetAction::Nod, PRI_NORMAL),
            PetEvent::MenuShake => self.enqueue(PetAction::Shake, PRI_FORCE),
            PetEvent::MenuReset => self.enqueue(PetAction::ResetPosition, PRI_CONTROL),
            PetEvent::MenuQuit => self.enqueue(PetAction::Quit, PRI_CONTROL),
            _ => {}
        }
    }

    fn enqueue(&mut self, action: PetAction, priority: i32) {
        // 控制类：插队到最前，立即执行。
        if action.is_control() {
            self.queue.push_front((action, priority));
            return;
        }
        // 避免队列里堆积重复的同类用户动作（连续点击去重）。
        if self.queue.iter().any(|(a, _)| *a == action) {
            return;
        }
        // 队列长度上限，防止极端情况无界增长。
        const MAX_QUEUE: usize = 8;
        if self.queue.len() >= MAX_QUEUE {
            return;
        }
        self.queue.push_back((action, priority));
    }

    /// 每帧调用，返回本帧应启动的动作（若有）。
    pub fn tick(&mut self, dt: f32, ctx: &TickContext) -> Option<PetAction> {
        // 计时器
        if self.cooldown > 0.0 {
            self.cooldown = (self.cooldown - dt).max(0.0);
        }
        if self.next_blink_in > 0.0 {
            self.next_blink_in -= dt;
        }
        if self.next_random_in > 0.0 {
            self.next_random_in -= dt;
        }

        // 当前动作完成检测
        if let Some(run) = &mut self.current {
            run.elapsed += dt;
            if ctx.is_motion_busy {
                run.seen_busy = true;
            } else if run.seen_busy {
                // 曾经忙过，现在不忙 → 动作结束
                self.cooldown = self.idle.action_cooldown;
                self.current = None;
            } else if run.elapsed > START_GRACE {
                // 从未变为忙（start 失败）→ 放弃
                self.cooldown = self.idle.action_cooldown;
                self.current = None;
            }
        }

        // 交互阻塞：只允许控制类动作插队。
        if ctx.interaction_blocked {
            if let Some(front) = self.queue.front() {
                if front.0.is_control() {
                    return self.queue.pop_front().map(|(a, _)| a);
                }
            }
            return None;
        }

        // 当前有动作在跑：控制类可抢占（立即返回，不排队）。
        if self.current.is_some() {
            if let Some(front) = self.queue.front() {
                if front.0.is_control() {
                    return self.queue.pop_front().map(|(a, _)| a);
                }
            }
            return None;
        }

        // 无当前动作：控制类优先插队。
        if let Some(front) = self.queue.front() {
            if front.0.is_control() {
                return self.queue.pop_front().map(|(a, _)| a);
            }
        }

        // 用户动作（按优先级：Shake(3) 优先于 Nod(2)）。
        if !self.queue.is_empty() {
            // 选择队列中优先级最高的
            let best_idx = self
                .queue
                .iter()
                .enumerate()
                .max_by_key(|(_, (_, p))| *p)
                .map(|(i, _)| i)
                .unwrap();
            let (action, priority) = self.queue.remove(best_idx).unwrap();
            self.current = Some(RunningAction {
                action,
                priority,
                elapsed: 0.0,
                seen_busy: false,
            });
            return Some(action);
        }

        // 空闲随机行为（冷却结束后）
        if self.cooldown <= 0.0 {
            // Blink：定时触发
            if self.next_blink_in <= 0.0 {
                self.next_blink_in = self
                    .rng
                    .range(self.idle.blink_interval_min, self.idle.blink_interval_max);
                self.current = Some(RunningAction {
                    action: PetAction::Blink,
                    priority: 2,
                    elapsed: 0.0,
                    seen_busy: false,
                });
                return Some(PetAction::Blink);
            }
            // 随机 Nod / Shake
            if self.next_random_in <= 0.0 {
                self.next_random_in = self.idle.min_interval;
                let r = self.rng.next_f32();
                let shake_p = self.idle.shake_probability.clamp(0.0, 1.0);
                let nod_p = self.idle.nod_probability.clamp(0.0, 1.0);
                if r < shake_p {
                    self.current = Some(RunningAction {
                        action: PetAction::Shake,
                        priority: 2,
                        elapsed: 0.0,
                        seen_busy: false,
                    });
                    return Some(PetAction::Shake);
                } else if r < shake_p + nod_p {
                    self.current = Some(RunningAction {
                        action: PetAction::Nod,
                        priority: 2,
                        elapsed: 0.0,
                        seen_busy: false,
                    });
                    return Some(PetAction::Nod);
                }
            }
        }

        None
    }
}
