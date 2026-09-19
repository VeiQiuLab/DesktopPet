//! ExpressionMapper：把助手文本映射为表达（保守规则）。
//!
//! **绝不**让 LLM 原始输出直接决定 motion / command / duration / priority。
//! 无法确定时：Text only。

use pet_protocol::{Motion, Priority};

pub struct Mapped {
    pub text: String,
    pub motion: Option<Motion>,
    pub priority: Priority,
}

/// 保守映射：普通回复 → Text；明确的疑问语气 → 可选 Nod；极少量模式 → Shake。
pub fn map(text: &str) -> Mapped {
    let t = text.trim();
    let motion = decide_motion(t);
    Mapped {
        text: t.to_string(),
        motion,
        // AI 回复视为普通/系统优先级
        priority: Priority::Normal,
    }
}

fn decide_motion(text: &str) -> Option<Motion> {
    // 明确的"摇头"语义（极少）
    if text.contains("不要晃") || text.contains("别摇") || text.contains("不要摇") {
        return Some(Motion::Shake);
    }
    // 明确的疑问语气 → Nod（轻微点头）
    let ends_question = text.ends_with('？') || text.ends_with('?');
    let short = text.chars().count() <= 12;
    if ends_question && short {
        return Some(Motion::Nod);
    }
    // 其余：不动作，避免乱动
    None
}
