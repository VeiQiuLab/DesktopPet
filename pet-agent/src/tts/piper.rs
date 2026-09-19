//! Piper 本地 TTS Provider（调用官方 piper-tts CLI）。
//!
//! 官方运行时为 `python -m piper`（piper-tts 包）。配置中：
//! - `exe`   = python 解释器路径（项目私有 venv）
//! - `model` = voice .onnx 路径
//! - `config`= voice .onnx.json 路径
//! - `data_dir` 可选（本实现直接用绝对 model/config，无需 data-dir）
//!
//! 安全：Command 独立参数（无 shell 拼接）；文本走 stdin（UTF-8）；
//! 超时 kill + wait；临时 WAV 用后即删。

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

    /// 诊断：检查 exe / model / config，并做一次极短 synthesis。
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
            lines.push("  -> MISSING".into());
        }
        // 极短 synthesis
        match self.synthesize_impl("测试") {
            Ok(a) => lines.push(format!(
                "short synthesis: ok ({} samples, {} Hz)",
                a.pcm_i16.len(),
                a.sample_rate
            )),
            Err(e) => lines.push(format!("short synthesis: FAILED ({e})")),
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

    fn validate(&self) -> Result<(), String> {
        if self.cfg.exe.trim().is_empty() {
            return Err("piper runtime (exe/python) not configured".into());
        }
        if !std::path::Path::new(&self.cfg.exe).is_file() {
            return Err(format!("piper runtime not found: {}", self.cfg.exe));
        }
        if self.cfg.model.trim().is_empty() {
            return Err("piper model not configured".into());
        }
        if !std::path::Path::new(&self.cfg.model).is_file() {
            return Err(format!("piper model not found: {}", self.cfg.model));
        }
        Ok(())
    }

    pub fn synthesize_impl(&self, text: &str) -> Result<AudioOutput, String> {
        self.validate()?;
        if text.trim().is_empty() {
            return Ok(AudioOutput::empty());
        }

        let out_path = temp::unique_path("wav");
        let out_str = out_path.to_string_lossy().to_string();

        // python -m piper -m <model> -c <config> -f <out>
        let mut cmd = Command::new(&self.cfg.exe);
        cmd.arg("-m").arg("piper");
        cmd.arg("-m").arg(&self.cfg.model);
        let cfg_path = self.resolve_config_path();
        if cfg_path.is_file() {
            cmd.arg("-c").arg(&cfg_path);
        }
        cmd.arg("-f").arg(&out_str);
        // 强制 Python 以 UTF-8 处理 stdin（Windows 默认按 locale/cp936）
        cmd.env("PYTHONIOENCODING", "utf-8");
        cmd.env("PYTHONUTF8", "1");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        // 不弹出控制台窗口（CREATE_NO_WINDOW = 0x08000000）
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn piper failed: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.write_all(b"\n");
        }

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
                let err_text = child
                    .stderr
                    .take()
                    .and_then(|mut e| {
                        use std::io::Read;
                        let mut buf = String::new();
                        e.read_to_string(&mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                let _ = std::fs::remove_file(&out_path);
                crate::log_line(&format!("piper stderr: {err_text}"));
                Err(format!("piper exited with code {:?}", s.code()))
            }
            Some(_) => {
                let bytes = std::fs::read(&out_path).map_err(|e| {
                    let _ = std::fs::remove_file(&out_path);
                    format!("read piper output failed: {e}")
                })?;
                let _ = std::fs::remove_file(&out_path);
                let audio = crate::tts::sapi::parse_wav(&bytes)?;
                if audio.is_empty() {
                    return Err("piper produced empty audio".into());
                }
                Ok(audio)
            }
        }
    }
}
