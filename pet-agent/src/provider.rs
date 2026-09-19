//! AI Provider 抽象 + Mock + OpenAI-compatible。

use crate::config::AgentConfig;
use crate::context::ChatMessage;

pub trait Provider {
    fn name(&self) -> &str;
    /// 生成回复。失败返回错误字符串（不 panic）。
    fn generate(&self, messages: &[ChatMessage], user_text: &str) -> Result<String, String>;
}

/// 根据名称构造 provider。
pub fn make(name: &str, cfg: &AgentConfig) -> Box<dyn Provider> {
    match name {
        "mock" => Box::new(MockProvider),
        "openai_compatible" => Box::new(OpenAiCompatibleProvider::new(cfg)),
        other => {
            eprintln!("[pet-agent] unknown provider '{other}', falling back to mock");
            Box::new(MockProvider)
        }
    }
}

// ---------- Mock ----------

pub struct MockProvider;

impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn generate(&self, _messages: &[ChatMessage], user_text: &str) -> Result<String, String> {
        let t = user_text.trim();
        let reply = if t.is_empty() {
            "……".to_string()
        } else if t.contains('?') || t.contains('？') {
            format!("嗯？{t}")
        } else if t == "你好" || t == "hi" || t == "hello" {
            "你好。".to_string()
        } else {
            format!("（mock）我听到了：{t}")
        };
        Ok(reply)
    }
}

// ---------- OpenAI-compatible ----------

pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key: Option<String>,
    timeout_secs: u64,
}

impl OpenAiCompatibleProvider {
    pub fn new(cfg: &AgentConfig) -> Self {
        OpenAiCompatibleProvider {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            api_key: cfg.api_key.clone(),
            timeout_secs: cfg.timeout_secs,
        }
    }
}

impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai_compatible"
    }

    fn generate(&self, messages: &[ChatMessage], _user_text: &str) -> Result<String, String> {
        if self.base_url.is_empty() {
            return Err("base_url not configured".into());
        }
        if self.model.is_empty() {
            return Err("model not configured".into());
        }

        let url = format!("{}/chat/completions", self.base_url);
        let msgs: Vec<_> = messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();
        let body = serde_json::json!({
            "model": self.model,
            "messages": msgs,
            "stream": false,
        });

        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build();
        let mut req = agent.post(&url).set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            if !key.is_empty() {
                req = req.set("Authorization", &format!("Bearer {key}"));
            }
        }

        let resp = req.send_json(body).map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("http status {code}"),
            ureq::Error::Transport(t) => format!("transport: {t}"),
        })?;

        let value: serde_json::Value = resp
            .into_json()
            .map_err(|e| format!("malformed response: {e}"))?;
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| "empty response".to_string())?;
        if content.trim().is_empty() {
            return Err("empty response".into());
        }
        Ok(content.trim().to_string())
    }
}
