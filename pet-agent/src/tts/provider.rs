//! TtsProvider 抽象 + Null / Mock 实现。
//!
//! 业务层只依赖本 trait；不出现 `if provider == ...` 分支。
//! 未来 SAPI / Edge TTS / Piper / GPT-SoVITS 等只需新增 impl。

use crate::config::TtsConfig;

pub trait TtsProvider: Send {
    fn name(&self) -> &str;
    /// 合成语音，返回统一 AudioOutput。
    fn synthesize(&self, text: &str) -> Result<crate::tts::audio::AudioOutput, String>;
}

pub fn make(cfg: &TtsConfig) -> Box<dyn TtsProvider> {
    match cfg.provider.as_str() {
        "null" => Box::new(NullTtsProvider),
        "mock" => Box::new(MockTtsProvider),
        "sapi" => Box::new(SapiTtsProvider::new(cfg)),
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
    fn synthesize(&self, _text: &str) -> Result<crate::tts::audio::AudioOutput, String> {
        Ok(crate::tts::audio::AudioOutput::empty())
    }
}

/// Windows SAPI 本地语音。
pub struct SapiTtsProvider {
    inner: crate::tts::sapi::SapiProvider,
}

impl SapiTtsProvider {
    pub fn new(cfg: &TtsConfig) -> Self {
        SapiTtsProvider {
            inner: crate::tts::sapi::SapiProvider::new(cfg.voice.clone(), cfg.rate, cfg.volume),
        }
    }
}

impl TtsProvider for SapiTtsProvider {
    fn name(&self) -> &str {
        "sapi"
    }
    fn synthesize(&self, text: &str) -> Result<crate::tts::audio::AudioOutput, String> {
        self.inner.synthesize_impl(text)
    }
}

/// 生成一段静音 WAV（用于验证链路），不依赖任何外部服务。
pub struct MockTtsProvider;

impl TtsProvider for MockTtsProvider {
    fn name(&self) -> &str {
        "mock"
    }
    fn synthesize(&self, text: &str) -> Result<crate::tts::audio::AudioOutput, String> {
        crate::log_line(&format!(
            "mock tts: synthesized {} chars",
            text.chars().count()
        ));
        // 生成 ~0.6s 静音 16-bit mono 16kHz
        let n = (16000.0 * 0.6) as usize;
        Ok(crate::tts::audio::AudioOutput {
            pcm_i16: vec![0i16; n],
            sample_rate: 16000,
            channels: 1,
            bits_per_sample: 16,
        })
    }
}
