//! AI worker 线程：HTTP 请求绝不阻塞 UI 线程。
//!
//! UI 线程 → request channel → worker → Provider → result channel → UI 线程。
//! 取消：用递增的 generation 标记；结果回来时若 generation 不匹配则丢弃。

use std::sync::mpsc::{channel, Receiver, Sender};

use crate::config::AgentConfig;
use crate::context::ChatMessage;
use crate::provider::{self, Provider};

pub struct AiRequest {
    pub generation: u64,
    pub messages: Vec<ChatMessage>,
    pub user_text: String,
}

pub struct AiResult {
    pub generation: u64,
    pub outcome: Result<String, String>,
    pub elapsed_ms: u128,
}

pub struct AiWorker {
    tx: Sender<AiRequest>,
    rx: Receiver<AiResult>,
}

impl AiWorker {
    pub fn start(cfg: AgentConfig, provider_override: Option<String>) -> Self {
        let (req_tx, req_rx) = channel::<AiRequest>();
        let (res_tx, res_rx) = channel::<AiResult>();

        std::thread::Builder::new()
            .name("ai-worker".into())
            .spawn(move || {
                let name = provider_override.as_deref().unwrap_or(&cfg.provider);
                let provider: Box<dyn Provider> = provider::make(name, &cfg);
                for req in req_rx {
                    let started = std::time::Instant::now();
                    let outcome = provider.generate(&req.messages, &req.user_text);
                    let elapsed_ms = started.elapsed().as_millis();
                    if res_tx
                        .send(AiResult {
                            generation: req.generation,
                            outcome,
                            elapsed_ms,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("spawn ai-worker");

        AiWorker {
            tx: req_tx,
            rx: res_rx,
        }
    }

    pub fn submit(&self, req: AiRequest) -> bool {
        self.tx.send(req).is_ok()
    }

    pub fn try_recv(&self) -> Option<AiResult> {
        self.rx.try_recv().ok()
    }

    /// provider 名称（用于状态显示）。
    pub fn provider_name(cfg: &AgentConfig, override_name: Option<&str>) -> String {
        override_name.unwrap_or(&cfg.provider).to_string()
    }
}
