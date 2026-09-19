//! Piper 本地 TTS Provider（调用已存在的 piper.exe + voice model）。
//!
//! 安全：
//! - 用 `Command` 独立参数，绝不 shell 拼接；
//! - 文本走 stdin（避免 command line 注入 / 引号 / 长度问题）；
//! - 超时后 kill + wait；临时文件用后即删。
//!
//! 链路：Piper → WAV → AudioOutput → Envelope → Playback + LipSync（不绕过 Playback）。

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::PiperConfig;
use crate::tts::audio::AudioOutput;
use crate::tts::temp;

pub struct PiperProvider {
    cfg: PiperConfig,
    timeout_secs: u64,
}

impl PiperProvider {
    pub fn new(cfg: &PiperConfig, timeout_secs: u64) -> Self {
        PiperProvider {
            cfg: cfg.clone(),
            timeout_secs,
        }
    }

    /// 诊断：检查 exe / model / config 是否存在，并做一次极短 synthesis。
    pub fn check(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("exe: {}", self.cfg.exe));
        if self.cfg.exe.is_empty() {
            lines.push("  -> exe not configured".into());
        } else if std::path::Path::new(&self.cfg.exe).is_file() {
            lines.push("  -> ok".into());
        } else {
            lines.push("  -> MISSING".into());
        }
        lines.push(format!("model: {}", self.cfg.model));
        if self.cfg.model.is_empty() {
            lines.push("  -> model not configured".into());
        } else if std::path::Path::new(&self.cfg.model).is_file() {
            lines.push("  -> ok".into());
        } else {
            lines.push("  -> MISSING".into());
        }
        let cfg_path = self.resolve_config_path();
        lines.push(format!("config: {}", cfg_path.display()));
        if cfg_path.is_file() {
            lines.push("  -> ok".into());
        } else {
            lines.push("  -> missing (may be optional)".into());
        }
        lines
    }

    fn resolve_config_path(&self) -> std::path::PathBuf {
        if !self.cfg.config.is_empty() {
            std::path::PathBuf::from(&self.cfg.config)
        } else {
            std::path::PathBuf::from(format!("{}.json", self.cfg.model))
        }
    }

    /// 校验配置，返回明确错误。
    fn validate(&self) -> Result<(), String> {
        if self.cfg.exe.trim().is_empty() {
            return Err("piper exe not configured".into());
        }
        if !std::path::Path::new(&self.cfg.exe).is_file() {
            return Err(format!("piper exe not found: {}", self.cfg.exe));
        }
        if self.cfg.model.trim().is_empty() {
            return Err("piper model not configured".into());
        }
        if !std::path::Path::new(&self.cfg.model).is_file() {
            return Err(format!("piper model not found: {}", self.cfg.model));
        }
        Ok(())
    }

    /// 合成到内存 AudioOutput。
    pub fn synthesize_impl(&self, text: &str) -> Result<AudioOutput, String> {
        self.validate()?;
        if text.trim().is_empty() {
            return Ok(AudioOutput::empty());
        }

        let out_path = temp::unique_path("wav");
        let out_str = out_path.to_string_lossy().to_string();

        // 独立参数（无 shell）
        let mut cmd = Command::new(&self.cfg.exe);
        cmd.arg("--model")
            .arg(&self.cfg.model)
            .arg("--output_file")
            .arg(&out_str)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn piper failed: {e}"))?;

        // 文本走 stdin（UTF-8）
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.write_all(b"\n");
            // 关闭 stdin 触发处理
        }

        // 轮询等待 + 超时
        let deadline = Instant::now() + Duration::from_secs(self.timeout_secs.max(1));
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&out_path);
                    return Err(format!("piper wait failed: {e}"));
                }
            }
        };

        match status {
            None => {
                let _ = std::fs::remove_file(&out_path);
                Err(format!("piper timeout after {}s", self.timeout_secs))
            }
            Some(s) if !s.success() => {
                let _ = std::fs::remove_file(&out_path);
                Err(format!("piper exited with code {:?}", s.code()))
            }
            Some(_) => {
                let bytes = std::fs::read(&out_path).map_err(|e| {
                    let _ = std::fs::remove_file(&out_path);
                    format!("read piper output failed: {e}")
                })?;
                let _ = std::fs::remove_file(&out_path);
                let audio = parse_wav(&bytes)?;
                if audio.is_empty() {
                    return Err("piper produced empty audio".into());
                }
                Ok(audio)
            }
        }
    }
}

/// 解析 16-bit PCM WAV → AudioOutput（与 sapi 共用逻辑）。
pub fn parse_wav(bytes: &[u8]) -> Result<AudioOutput, String> {
    crate::tts::sapi::parse_wav(bytes)
}
