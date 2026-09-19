//! Expression Protocol v1 与 Command Protocol。
//!
//! 所有外部输入视为不可信：限制消息大小、文本长度、动作白名单、版本号。

use serde::{Deserialize, Serialize};

/// 协议版本。
pub const PROTOCOL_VERSION: u32 = 1;
/// 单条消息最大字节数。
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// 文本最大 Unicode 字符数。
pub const MAX_TEXT_CHARS: usize = 1000;
/// 展示时长上下限（毫秒）。
pub const MIN_DURATION_MS: u64 = 500;
pub const MAX_DURATION_MS: u64 = 30_000;

/// 客户端请求。
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    #[serde(rename = "expression")]
    Expression(ExpressionReq),
    #[serde(rename = "command")]
    Command(CommandReq),
}

#[derive(Debug, Deserialize)]
pub struct ExpressionReq {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub motion: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandReq {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub id: Option<String>,
    pub command: String,
    #[serde(default)]
    pub character: Option<String>,
}

/// 解析后的、已校验的内部表达（供主线程消费）。
#[derive(Debug, Clone)]
pub struct ValidatedExpression {
    pub id: Option<String>,
    pub text: Option<String>,
    pub motion: Option<SemanticMotion>,
    pub priority: SemPriority,
    pub duration_ms: Option<u64>,
}

/// 语义动作（外部只能使用这些）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticMotion {
    Idle,
    Blink,
    Nod,
    Shake,
}

impl SemanticMotion {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Idle" | "idle" => Some(SemanticMotion::Idle),
            "Blink" | "blink" => Some(SemanticMotion::Blink),
            "Nod" | "nod" => Some(SemanticMotion::Nod),
            "Shake" | "shake" => Some(SemanticMotion::Shake),
            _ => None,
        }
    }

    pub fn to_action(self) -> crate::behavior::PetAction {
        match self {
            SemanticMotion::Idle => crate::behavior::PetAction::Idle,
            SemanticMotion::Blink => crate::behavior::PetAction::Blink,
            SemanticMotion::Nod => crate::behavior::PetAction::Nod,
            SemanticMotion::Shake => crate::behavior::PetAction::Shake,
        }
    }
}

/// 外部优先级（受限枚举，禁止任意整数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemPriority {
    Low,
    Normal,
    High,
}

impl SemPriority {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" | "Low" => Some(SemPriority::Low),
            "normal" | "Normal" => Some(SemPriority::Normal),
            "high" | "High" => Some(SemPriority::High),
            _ => None,
        }
    }

    pub fn to_presentation(self) -> crate::presentation::Priority {
        match self {
            SemPriority::Low => crate::presentation::Priority::Idle,
            SemPriority::Normal => crate::presentation::Priority::System,
            SemPriority::High => crate::presentation::Priority::User,
        }
    }
}

/// 校验后的控制命令。
#[derive(Debug, Clone)]
pub enum ValidatedCommand {
    Show,
    Hide,
    ResetPosition,
    DismissBubble,
    Character(String),
}

/// 校验错误。
#[derive(Debug)]
pub enum ValidateError {
    UnsupportedVersion(u32),
    Empty,
    TextTooLong,
    UnknownMotion(String),
    UnknownCommand(String),
    UnknownPriority(String),
}

impl std::fmt::Display for ValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidateError::UnsupportedVersion(v) => {
                write!(f, "unsupported protocol version: {v}")
            }
            ValidateError::Empty => write!(f, "request must contain text or motion"),
            ValidateError::TextTooLong => {
                write!(f, "text exceeds {MAX_TEXT_CHARS} characters")
            }
            ValidateError::UnknownMotion(m) => write!(f, "unknown motion: {m}"),
            ValidateError::UnknownCommand(c) => write!(f, "unknown command: {c}"),
            ValidateError::UnknownPriority(p) => write!(f, "unknown priority: {p}"),
        }
    }
}

/// 解析并校验原始 JSON → 内部表达。
pub fn parse_request(json: &str) -> Result<ValidatedRequest, ValidateError> {
    let req: Request = serde_json::from_str(json).map_err(|_| ValidateError::Empty)?; // JSON 非法 → 归为 Empty（外部只见通用错误）
    match req {
        Request::Expression(e) => {
            if e.version != 0 && e.version != PROTOCOL_VERSION {
                return Err(ValidateError::UnsupportedVersion(e.version));
            }
            // 文本长度
            if let Some(t) = &e.text {
                if t.chars().count() > MAX_TEXT_CHARS {
                    return Err(ValidateError::TextTooLong);
                }
            }
            // 动作
            let motion = match &e.motion {
                Some(m) => Some(
                    SemanticMotion::parse(m)
                        .ok_or_else(|| ValidateError::UnknownMotion(m.clone()))?,
                ),
                None => None,
            };
            // 优先级
            let priority = match &e.priority {
                Some(p) => SemPriority::parse(p)
                    .ok_or_else(|| ValidateError::UnknownPriority(p.clone()))?,
                None => SemPriority::Normal,
            };
            // 至少要有 text 或 motion
            let has_text = e
                .text
                .as_ref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);
            if !has_text && motion.is_none() {
                return Err(ValidateError::Empty);
            }
            // duration clamp
            let duration_ms = e
                .duration_ms
                .map(|d| d.clamp(MIN_DURATION_MS, MAX_DURATION_MS));
            Ok(ValidatedRequest::Expression(ValidatedExpression {
                id: e.id,
                text: e.text.filter(|t| !t.trim().is_empty()),
                motion,
                priority,
                duration_ms,
            }))
        }
        Request::Command(c) => {
            if c.version != 0 && c.version != PROTOCOL_VERSION {
                return Err(ValidateError::UnsupportedVersion(c.version));
            }
            let cmd = match c.command.as_str() {
                "show" => ValidatedCommand::Show,
                "hide" => ValidatedCommand::Hide,
                "reset_position" => ValidatedCommand::ResetPosition,
                "dismiss_bubble" => ValidatedCommand::DismissBubble,
                "character" => ValidatedCommand::Character(c.character.clone().unwrap_or_default()),
                other => return Err(ValidateError::UnknownCommand(other.to_string())),
            };
            Ok(ValidatedRequest::Command {
                id: c.id,
                command: cmd,
            })
        }
    }
}

#[derive(Debug, Clone)]
pub enum ValidatedRequest {
    Expression(ValidatedExpression),
    Command {
        id: Option<String>,
        command: ValidatedCommand,
    },
}

/// 响应。
#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: Option<String>) -> Self {
        Response {
            ok: true,
            id,
            accepted: Some(true),
            error: None,
        }
    }
    pub fn err(id: Option<String>, error: impl Into<String>) -> Self {
        Response {
            ok: false,
            id,
            accepted: None,
            error: Some(error.into()),
        }
    }
}
