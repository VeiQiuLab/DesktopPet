//! Behavior 层：将底层输入事件（PetEvent）转换为语义动作（PetAction）。
//!
//! 职责边界：
//! - window.rs 只负责采集输入，产生 PetEvent；
//! - 本模块将 PetEvent 转换为 PetAction（+ 优先级）；
//! - character 层将语义动作映射到具体 motion group；
//! - Cubism shim 执行实际播放。

/// 由窗口层采集的语义事件。
#[derive(Debug, Clone, Copy)]
pub enum PetEvent {
    PointerEnter,
    PointerLeave,
    LeftClick,
    DoubleClick,
    DragStart,
    DragEnd,
    RightClick,
    /// 右键菜单选择「点头」。
    MenuNod,
    /// 右键菜单选择「摇头」。
    MenuShake,
    /// 右键菜单选择「重置位置」。
    MenuReset,
    /// 右键菜单选择「退出」。
    MenuQuit,
}

/// 语义动作（与底层 motion group 解耦，允许换角色重新映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PetAction {
    Idle,
    Blink,
    Nod,
    Shake,
    /// 将桌宠移回默认位置。
    ResetPosition,
    /// 退出应用。
    Quit,
}

impl PetAction {
    /// 语义名。character 层用它查找实际的 motion group。
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
}

/// 最小动作调度器：缓存一个待触发的动作。
///
/// 规则：
/// - 同一时刻只保留一个 pending 动作；
/// - 高优先级动作可覆盖低优先级；
/// - take_pending 之后清空（由主循环消费）。
pub struct BehaviorController {
    pending: Option<(PetAction, i32)>,
}

impl BehaviorController {
    pub fn new() -> Self {
        BehaviorController { pending: None }
    }

    /// 处理一个输入事件，必要时更新 pending 动作。
    pub fn handle(&mut self, ev: PetEvent) {
        match ev {
            PetEvent::LeftClick => self.schedule(PetAction::Nod, 2),
            PetEvent::DoubleClick => self.schedule(PetAction::Shake, 3),
            PetEvent::MenuNod => self.schedule(PetAction::Nod, 2),
            PetEvent::MenuShake => self.schedule(PetAction::Shake, 3),
            // 控制类动作使用最高优先级，确保不被动作抢占覆盖
            PetEvent::MenuReset => self.schedule(PetAction::ResetPosition, 10),
            PetEvent::MenuQuit => self.schedule(PetAction::Quit, 10),
            // 拖拽/悬停目前不产生动作
            _ => {}
        }
    }

    fn schedule(&mut self, action: PetAction, priority: i32) {
        if let Some((_, p)) = self.pending {
            if priority <= p {
                return;
            }
        }
        self.pending = Some((action, priority));
    }

    /// 取出待触发动作（如果有）。
    pub fn take_pending(&mut self) -> Option<(PetAction, i32)> {
        self.pending.take()
    }
}
