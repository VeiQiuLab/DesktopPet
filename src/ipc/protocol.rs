//! DesktopPet 侧的 IPC 协议适配层。
//!
//! 真正的 wire schema 定义在共享 crate `pet-protocol` 中（模型无关）。
//! 本模块把 `pet-protocol` 的中间表示转换为 DesktopPet 内部类型
//! （`PetAction` / `presentation::Priority`），并保留响应构造。

pub use pet_protocol::MAX_MESSAGE_BYTES;
pub use pet_protocol::{Motion as SemanticMotion, Priority as SemPriority, Response, WireCommand};

use pet_protocol::WireRequest as Wire;

/// 校验后的请求（DesktopPet 内部表示）。
#[derive(Debug, Clone)]
pub enum ValidatedRequest {
    Expression(ValidatedExpression),
    Command {
        id: Option<String>,
        command: ValidatedCommand,
    },
}

#[derive(Debug, Clone)]
pub struct ValidatedExpression {
    pub id: Option<String>,
    pub text: Option<String>,
    pub motion: Option<SemanticMotion>,
    pub priority: SemPriority,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ValidatedCommand {
    Show,
    Hide,
    ResetPosition,
    DismissBubble,
    Character(String),
}

/// 语义动作 → DesktopPet 内部 PetAction。
pub fn motion_to_action(m: SemanticMotion) -> crate::behavior::PetAction {
    match m {
        SemanticMotion::Idle => crate::behavior::PetAction::Idle,
        SemanticMotion::Blink => crate::behavior::PetAction::Blink,
        SemanticMotion::Nod => crate::behavior::PetAction::Nod,
        SemanticMotion::Shake => crate::behavior::PetAction::Shake,
    }
}

/// 外部优先级 → Presentation 优先级。
pub fn priority_to_presentation(p: SemPriority) -> crate::presentation::Priority {
    match p {
        SemPriority::Low => crate::presentation::Priority::Idle,
        SemPriority::Normal => crate::presentation::Priority::System,
        SemPriority::High => crate::presentation::Priority::User,
    }
}

/// 解析并校验原始 JSON → DesktopPet 内部表示。
pub fn parse_request(json: &str) -> Result<ValidatedRequest, String> {
    let wire = pet_protocol::parse(json).map_err(|e| e.to_string())?;
    Ok(match wire {
        Wire::Expression {
            id,
            text,
            motion,
            priority,
            duration_ms,
        } => ValidatedRequest::Expression(ValidatedExpression {
            id,
            text,
            motion,
            priority,
            duration_ms,
        }),
        Wire::Command { id, command } => {
            let cmd = match command {
                WireCommand::Show => ValidatedCommand::Show,
                WireCommand::Hide => ValidatedCommand::Hide,
                WireCommand::ResetPosition => ValidatedCommand::ResetPosition,
                WireCommand::DismissBubble => ValidatedCommand::DismissBubble,
                WireCommand::Character(c) => ValidatedCommand::Character(c),
            };
            ValidatedRequest::Command { id, command: cmd }
        }
    })
}
