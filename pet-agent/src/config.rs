//! pet-agent 配置。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// 默认 provider 名："mock" | "openai_compatible"。
    #[serde(default = "default_provider")]
    pub provider: String,
    /// OpenAI-compatible base url，例如 http://127.0.0.1:8080/v1
    #[serde(default)]
    pub base_url: String,
    /// 模型名（由用户配置，不假定）。
    #[serde(default)]
    pub model: String,
    /// API key（本地可为空）。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 请求超时（秒）。
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// system prompt。
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// 保留的历史轮数上限。
    #[serde(default = "default_history_limit")]
    pub history_limit: usize,
    /// 气泡最大字符数。
    #[serde(default = "default_bubble_max")]
    pub bubble_max_chars: usize,
    /// 是否在日志中记录完整对话（默认 false）。
    #[serde(default)]
    pub log_conversation: bool,
}

fn default_provider() -> String {
    "mock".into()
}
fn default_timeout() -> u64 {
    60
}
fn default_history_limit() -> usize {
    12
}
fn default_bubble_max() -> usize {
    80
}
fn default_system_prompt() -> String {
    "你是桌面角色的对话层。回复尽量简短自然，通常 1～3 句话，适合显示在小小的桌面气泡里。不要输出 Markdown 或大段格式。".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            provider: default_provider(),
            base_url: String::new(),
            model: String::new(),
            api_key: None,
            timeout_secs: default_timeout(),
            system_prompt: default_system_prompt(),
            history_limit: default_history_limit(),
            bubble_max_chars: default_bubble_max(),
            log_conversation: false,
        }
    }
}

impl AgentConfig {
    /// 从 exe 同目录的 pet-agent.config.json 读取；不存在则用默认并写出。
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<AgentConfig>(&text) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[pet-agent] config parse error ({e}), using default");
                    AgentConfig::default()
                }
            },
            Err(_) => {
                let c = AgentConfig::default();
                if let Ok(text) = serde_json::to_string_pretty(&c) {
                    let _ = std::fs::write(&path, text);
                }
                c
            }
        }
    }
}

fn config_path() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    p.push("pet-agent.config.json");
    p
}
