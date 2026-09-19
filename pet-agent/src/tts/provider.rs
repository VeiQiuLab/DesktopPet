//! TtsProvider 抽象 + Null / Mock 实现。
//!
//! 业务层只依赖本 trait；不出现 `if provider == ...` 分支。
//! 未来 SAPI / Edge TTS / Piper / GPT-SoVITS 等只需新增 impl。

use crate::config::TtsConfig;

pub trait TtsProvider: Send {
    fn name(&self) -> &str;
    /// 合成语音，返回 WAV 字节（可为空表示无音频）。
    fn synthesize(&self, text: &str) -> Result<Vec<u8>, String>;
}

pub fn make(cfg: &TtsConfig) -> Box<dyn TtsProvider> {
    match cfg.provider.as_str() {
        "null" => Box::new(NullTtsProvider),
        "mock" => Box::new(MockTtsProvider),
        // 第一版：未知 provider 回退 mock（不崩溃）
        _ => {
            crate::log_line(&format!(
                "unknown tts provider '{}', falling back to mock",
                cfg.provider
            ));
            Box::new(MockTtsProvider)
        }
    }
}

/// 不产生语音。
pub struct NullTtsProvider;

impl TtsProvider for NullTtsProvider {
    fn name(&self) -> &str {
        "null"
    }
    fn synthesize(&self, _text: &str) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

/// 生成一段静音 WAV（用于验证链路），不依赖任何外部服务。
pub struct MockTtsProvider;

impl TtsProvider for MockTtsProvider {
    fn name(&self) -> &str {
        "mock"
    }
    fn synthesize(&self, text: &str) -> Result<Vec<u8>, String> {
        crate::log_line(&format!(
            "mock tts: synthesized {} chars",
            text.chars().count()
        ));
        // 生成 ~0.6s 静音 16-bit mono 16kHz WAV
        Ok(silent_wav(16000, 1, 16, 0.6))
    }
}

/// 生成静音 WAV 字节。
fn silent_wav(sample_rate: u32, channels: u16, bits: u16, seconds: f32) -> Vec<u8> {
    let num_samples = (sample_rate as f32 * seconds) as u32;
    let bytes_per_sample = (bits / 8) as u32;
    let data_size = num_samples * channels as u32 * bytes_per_sample;
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
    let block_align = channels * (bits / 8);

    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.resize(44 + data_size as usize, 0);
    out
}
