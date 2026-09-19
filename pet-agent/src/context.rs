//! 轻量对话上下文（短上下文，无 Memory / RAG / Embedding）。

use crate::config::AgentConfig;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

pub struct Conversation {
    system_prompt: String,
    history_limit: usize,
    turns: Vec<ChatMessage>,
}

impl Conversation {
    pub fn new(cfg: &AgentConfig) -> Self {
        Conversation {
            system_prompt: cfg.system_prompt.clone(),
            history_limit: cfg.history_limit.max(1),
            turns: Vec::new(),
        }
    }

    pub fn push_user(&mut self, text: &str) {
        self.turns.push(ChatMessage {
            role: "user",
            content: text.to_string(),
        });
        self.trim();
    }

    pub fn push_assistant(&mut self, text: &str) {
        self.turns.push(ChatMessage {
            role: "assistant",
            content: text.to_string(),
        });
        self.trim();
    }

    /// system + 最近 N 轮。
    pub fn messages(&self) -> Vec<ChatMessage> {
        let mut out = Vec::with_capacity(self.turns.len() + 1);
        out.push(ChatMessage {
            role: "system",
            content: self.system_prompt.clone(),
        });
        out.extend(self.turns.iter().cloned());
        out
    }

    fn trim(&mut self) {
        let max = self.history_limit * 2; // 每轮 user+assistant
        if self.turns.len() > max {
            let excess = self.turns.len() - max;
            self.turns.drain(0..excess);
        }
    }
}
