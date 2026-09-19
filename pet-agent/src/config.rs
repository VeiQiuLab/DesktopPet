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
    /// TTS 配置。
    #[serde(default)]
    pub tts: TtsConfig,
    /// Lip sync 配置。
    #[serde(default)]
    pub lip_sync: LipSyncConfig,
    /// 当前激活的 persona id。
    #[serde(default = "default_persona")]
    pub active_persona: String,
}

fn default_persona() -> String {
    "default".into()
}

/// Lip sync 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LipSyncConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_sample_hz")]
    pub sample_hz: f32,
    #[serde(default = "default_rate")]
    pub gain: f32,
    #[serde(default = "default_noise_floor")]
    pub noise_floor: f32,
    #[serde(default = "default_attack")]
    pub attack: f32,
    #[serde(default = "default_release")]
    pub release: f32,
    #[serde(default)]
    pub start_delay_ms: u32,
}

fn default_sample_hz() -> f32 {
    30.0
}
fn default_noise_floor() -> f32 {
    0.02
}
fn default_attack() -> f32 {
    0.7
}
fn default_release() -> f32 {
    0.25
}

impl Default for LipSyncConfig {
    fn default() -> Self {
        LipSyncConfig {
            enabled: true,
            sample_hz: default_sample_hz(),
            gain: default_rate(),
            noise_floor: default_noise_floor(),
            attack: default_attack(),
            release: default_release(),
            start_delay_ms: 0,
        }
    }
}

/// TTS 配置。默认 disabled，避免升级后突然出声。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_tts_provider")]
    pub provider: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default = "default_rate")]
    pub rate: f32,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_true")]
    pub speak_bubble_text_only: bool,
    /// Piper 专用配置。
    #[serde(default)]
    pub piper: PiperConfig,
    /// synthesis 超时（秒）。
    #[serde(default = "default_tts_timeout")]
    pub timeout_secs: u64,
}

/// Piper 本地 TTS 引擎配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PiperConfig {
    /// piper.exe 绝对路径。
    #[serde(default)]
    pub exe: String,
    /// voice 模型 .onnx 绝对路径。
    #[serde(default)]
    pub model: String,
    /// voice 配置 .onnx.json 绝对路径（可选；缺省从 model 推导）。
    #[serde(default)]
    pub config: String,
}

fn default_tts_timeout() -> u64 {
    30
}

fn default_tts_provider() -> String {
    "mock".into()
}
fn default_rate() -> f32 {
    1.0
}
fn default_volume() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}

impl Default for TtsConfig {
    fn default() -> Self {
        TtsConfig {
            enabled: false,
            provider: default_tts_provider(),
            voice: None,
            rate: default_rate(),
            volume: default_volume(),
            speak_bubble_text_only: true,
            piper: PiperConfig::default(),
            timeout_secs: default_tts_timeout(),
        }
    }
}

impl TtsConfig {
    /// 归一化（clamp rate/volume）。
    pub fn normalized(mut self) -> Self {
        self.rate = self.rate.clamp(0.5, 2.0);
        self.volume = self.volume.clamp(0.0, 1.0);
        self
    }
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
            tts: TtsConfig::default(),
            lip_sync: LipSyncConfig::default(),
            active_persona: default_persona(),
        }
    }
}

impl AgentConfig {
    /// 持久化到配置文件（用于 persona 切换）。
    pub fn save(&self) {
        let path = config_path();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, text);
        }
    }
}

/// 进程级 lip_sync 开关（由 cfg 初始化，供 TTS worker 读取）。
static LIP_SYNC_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_lip_sync_enabled(v: bool) {
    LIP_SYNC_ENABLED.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn lip_sync_enabled() -> bool {
    LIP_SYNC_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
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
