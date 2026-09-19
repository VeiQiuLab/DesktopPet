//! DesktopPet 与 pet-agent 共享的线协议（wire protocol）。
//!
//! 本 crate **只依赖 serde**，不含任何 Win32 / D3D / Cubism / Presentation 依赖。
//! 职责：定义请求 / 响应结构、枚举、校验与常量。
//! 语义映射（Motion → PetAction、Priority → Presentation Priority）由各自 Runtime 完成。

use serde::{Deserialize, Serialize};

/// 协议版本。
pub const PROTOCOL_VERSION: u32 = 1;
/// Named Pipe 名称（版本化）。
pub const PIPE_NAME: &str = r"\\.\pipe\DesktopPetExpression_v1";
/// 单条消息最大字节数。
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// 文本最大 Unicode 字符数。
pub const MAX_TEXT_CHARS: usize = 1000;
/// 展示时长上下限（毫秒）。
pub const MIN_DURATION_MS: u64 = 500;
pub const MAX_DURATION_MS: u64 = 30_000;

/// 语义动作（外部只能使用这些）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Idle,
    Blink,
    Nod,
    Shake,
}

impl Motion {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Idle" | "idle" => Some(Motion::Idle),
            "Blink" | "blink" => Some(Motion::Blink),
            "Nod" | "nod" => Some(Motion::Nod),
            "Shake" | "shake" => Some(Motion::Shake),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Motion::Idle => "Idle",
            Motion::Blink => "Blink",
            Motion::Nod => "Nod",
            Motion::Shake => "Shake",
        }
    }
}

/// 外部优先级（受限枚举，禁止任意整数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Normal,
    High,
}

impl Priority {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" | "Low" => Some(Priority::Low),
            "normal" | "Normal" => Some(Priority::Normal),
            "high" | "High" => Some(Priority::High),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
        }
    }
}

// ---------- 原始请求（反序列化） ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    #[serde(rename = "expression")]
    Expression(ExpressionRequest),
    #[serde(rename = "command")]
    Command(CommandRequest),
}

#[derive(Debug, Deserialize)]
pub struct ExpressionRequest {
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
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub id: Option<String>,
    pub command: String,
    #[serde(default)]
    pub character: Option<String>,
}

// ---------- 校验后的中间表示（平台无关） ----------

#[derive(Debug, Clone)]
pub enum WireRequest {
    Expression {
        id: Option<String>,
        text: Option<String>,
        motion: Option<Motion>,
        priority: Priority,
        duration_ms: Option<u64>,
    },
    Command {
        id: Option<String>,
        command: WireCommand,
    },
}

#[derive(Debug, Clone)]
pub enum WireCommand {
    Show,
    Hide,
    ResetPosition,
    DismissBubble,
    Character(String),
}

impl WireRequest {
    pub fn id(&self) -> Option<String> {
        match self {
            WireRequest::Expression { id, .. } => id.clone(),
            WireRequest::Command { id, .. } => id.clone(),
        }
    }
}

/// 校验错误。
#[derive(Debug)]
pub enum ValidateError {
    InvalidJson,
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
            ValidateError::InvalidJson => write!(f, "invalid json"),
            ValidateError::UnsupportedVersion(v) => write!(f, "unsupported protocol version: {v}"),
            ValidateError::Empty => write!(f, "request must contain text or motion"),
            ValidateError::TextTooLong => write!(f, "text exceeds {MAX_TEXT_CHARS} characters"),
            ValidateError::UnknownMotion(m) => write!(f, "unknown motion: {m}"),
            ValidateError::UnknownCommand(c) => write!(f, "unknown command: {c}"),
            ValidateError::UnknownPriority(p) => write!(f, "unknown priority: {p}"),
        }
    }
}

/// 解析并校验一条原始 JSON（平台无关）。
pub fn parse(text: &str) -> Result<WireRequest, ValidateError> {
    let req: Request = serde_json::from_str(text).map_err(|_| ValidateError::InvalidJson)?;
    match req {
        Request::Expression(e) => {
            if e.version != 0 && e.version != PROTOCOL_VERSION {
                return Err(ValidateError::UnsupportedVersion(e.version));
            }
            if let Some(t) = &e.text {
                if t.chars().count() > MAX_TEXT_CHARS {
                    return Err(ValidateError::TextTooLong);
                }
            }
            let motion = match &e.motion {
                Some(m) => Some(
                    Motion::parse(m).ok_or_else(|| ValidateError::UnknownMotion(m.clone()))?,
                ),
                None => None,
            };
            let priority = match &e.priority {
                Some(p) => {
                    Priority::parse(p).ok_or_else(|| ValidateError::UnknownPriority(p.clone()))?
                }
                None => Priority::Normal,
            };
            let has_text = e
                .text
                .as_ref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);
            if !has_text && motion.is_none() {
                return Err(ValidateError::Empty);
            }
            let duration_ms = e
                .duration_ms
                .map(|d| d.clamp(MIN_DURATION_MS, MAX_DURATION_MS));
            Ok(WireRequest::Expression {
                id: e.id,
                text: e.text.filter(|t| !t.trim().is_empty()),
                motion,
                priority,
                duration_ms,
            })
        }
        Request::Command(c) => {
            if c.version != 0 && c.version != PROTOCOL_VERSION {
                return Err(ValidateError::UnsupportedVersion(c.version));
            }
            let command = match c.command.as_str() {
                "show" => WireCommand::Show,
                "hide" => WireCommand::Hide,
                "reset_position" => WireCommand::ResetPosition,
                "dismiss_bubble" => WireCommand::DismissBubble,
                "character" => WireCommand::Character(c.character.clone().unwrap_or_default()),
                other => return Err(ValidateError::UnknownCommand(other.to_string())),
            };
            Ok(WireRequest::Command {
                id: c.id,
                command,
            })
        }
    }
}

// ---------- 响应 ----------

#[derive(Debug, Serialize, Deserialize)]
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

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"ok\":false}".to_string())
    }
}

// ---------- 客户端构造辅助 ----------

/// 构造 expression 请求 JSON。
pub fn build_expression(
    text: Option<&str>,
    motion: Option<Motion>,
    priority: Priority,
    duration_ms: Option<u64>,
    id: Option<&str>,
) -> String {
    let mut obj = serde_json::json!({
        "version": PROTOCOL_VERSION,
        "type": "expression",
        "priority": priority.name(),
    });
    if let Some(t) = text {
        obj["text"] = serde_json::Value::String(t.to_string());
    }
    if let Some(m) = motion {
        obj["motion"] = serde_json::Value::String(m.name().to_string());
    }
    if let Some(d) = duration_ms {
        obj["duration_ms"] = serde_json::Value::Number(d.into());
    }
    if let Some(i) = id {
        obj["id"] = serde_json::Value::String(i.to_string());
    }
    obj.to_string()
}

/// 构造 command 请求 JSON。
pub fn build_command(command: &str, character: Option<&str>) -> String {
    let mut obj = serde_json::json!({
        "version": PROTOCOL_VERSION,
        "type": "command",
        "command": command,
    });
    if let Some(c) = character {
        obj["character"] = serde_json::Value::String(c.to_string());
    }
    obj.to_string()
}
