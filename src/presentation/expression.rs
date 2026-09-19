//! 表达请求（PetExpression）。

/// 表达优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum Priority {
    /// 自动待机文本（最低）。
    Idle = 0,
    /// 系统提示。
    System = 1,
    /// 用户 / 未来 AI 主动消息（最高）。
    User = 2,
}

/// 一次「表达」请求。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PetExpression {
    /// 纯文本。
    Text {
        text: String,
        priority: Priority,
        /// 展示时长（秒）；None = 按文本长度估算。
        duration: Option<f32>,
    },
    /// 纯动作（无文字）。
    Motion {
        action: crate::behavior::PetAction,
        priority: Priority,
    },
    /// 文本 + 动作（逻辑关联，不要求帧同步）。
    TextAndMotion {
        text: String,
        action: crate::behavior::PetAction,
        priority: Priority,
        duration: Option<f32>,
    },
}

impl PetExpression {
    pub fn text(&self) -> Option<&str> {
        match self {
            PetExpression::Text { text, .. } => Some(text),
            PetExpression::TextAndMotion { text, .. } => Some(text),
            PetExpression::Motion { .. } => None,
        }
    }

    pub fn motion(&self) -> Option<crate::behavior::PetAction> {
        match self {
            PetExpression::Motion { action, .. } => Some(*action),
            PetExpression::TextAndMotion { action, .. } => Some(*action),
            PetExpression::Text { .. } => None,
        }
    }

    pub fn priority(&self) -> Priority {
        match self {
            PetExpression::Text { priority, .. } => *priority,
            PetExpression::Motion { priority, .. } => *priority,
            PetExpression::TextAndMotion { priority, .. } => *priority,
        }
    }

    pub fn duration(&self) -> Option<f32> {
        match self {
            PetExpression::Text { duration, .. } => *duration,
            PetExpression::TextAndMotion { duration, .. } => *duration,
            PetExpression::Motion { .. } => None,
        }
    }
}
