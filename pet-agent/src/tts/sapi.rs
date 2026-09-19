//! Windows SAPI 本地 TTS Provider（无需 API Key / 互联网）。
//!
//! 通过 COM ISpVoice 合成到临时 WAV 文件，解析为 AudioOutput。
//! 临时文件在 %TEMP% 下，用后即删；启动时清理过期文件。

use crate::tts::audio::AudioOutput;

pub struct SapiProvider {
    voice: Option<String>,
    rate: f32,
    volume: f32,
}

impl SapiProvider {
    pub fn new(voice: Option<String>, rate: f32, volume: f32) -> Self {
        SapiProvider {
            voice,
            rate,
            volume,
        }
    }

    pub fn list_voices() -> Vec<(String, String)> {
        voices_impl()
    }

    pub fn synthesize_impl(&self, text: &str) -> Result<AudioOutput, String> {
        synthesize_impl(text, self.voice.as_deref(), self.rate, self.volume)
    }
}

/// 临时目录。
fn temp_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push("DesktopPet");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// 启动时清理过期临时文件（> 1 小时）。
pub fn cleanup_temp() {
    let dir = temp_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let now = std::time::SystemTime::now();
        for e in entries.flatten() {
            if let Ok(meta) = e.metadata() {
                if let Ok(modified) = meta.modified() {
                    if now
                        .duration_since(modified)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                        > 3600
                    {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }
}

// ---------- Windows 实现 ----------

#[cfg(windows)]
fn voices_impl() -> Vec<(String, String)> {
    use windows::core::HSTRING;
    use windows::Win32::Media::Speech::{ISpVoice, SpVoice};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut out = Vec::new();
        let voice: Result<ISpVoice, _> = CoCreateInstance(&SpVoice, None, CLSCTX_ALL);
        if let Ok(v) = voice {
            if let Ok(token) = v.GetVoice() {
                let _ = token;
                out.push(("(default)".to_string(), "system default voice".to_string()));
            }
        }
        let _ = HSTRING::new();
        out
    }
}

#[cfg(not(windows))]
fn voices_impl() -> Vec<(String, String)> {
    Vec::new()
}

#[cfg(windows)]
fn synthesize_impl(
    text: &str,
    voice: Option<&str>,
    rate: f32,
    volume: f32,
) -> Result<AudioOutput, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Media::Speech::{
        ISpStream, ISpVoice, SpStream, SpVoice, SPFM_CREATE_ALWAYS,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };

    if text.trim().is_empty() {
        return Ok(AudioOutput::empty());
    }

    let wav_path = temp_dir().join(format!(
        "tts_{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let voice_obj: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)
            .map_err(|e| format!("CoCreateInstance(SpVoice): {e}"))?;
        let stream_obj: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL)
            .map_err(|e| format!("CoCreateInstance(SpStream): {e}"))?;

        // 绑定到文件（默认 WAV 格式）
        let path_wide: Vec<u16> = wav_path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        stream_obj
            .BindToFile(
                PCWSTR(path_wide.as_ptr()),
                SPFM_CREATE_ALWAYS,
                None,
                None,
                0,
            )
            .map_err(|e| format!("BindToFile: {e}"))?;

        voice_obj
            .SetOutput(&stream_obj, true)
            .map_err(|e| format!("SetOutput: {e}"))?;

        // rate: -10..10, volume: 0..100
        let r = ((rate.clamp(0.5, 2.0) - 1.0) * 10.0).round() as i32;
        let _ = voice_obj.SetRate(r.clamp(-10, 10));
        let _ = voice_obj.SetVolume((volume.clamp(0.0, 1.0) * 100.0) as u16);

        let _ = voice;
        let text_h = HSTRING::from(text);
        voice_obj
            .Speak(&text_h, 0, None)
            .map_err(|e| format!("Speak: {e}"))?;

        // flush：切换输出到默认音频设备，确保文件写完
        let _ = voice_obj.SetOutput(None::<&windows::core::IUnknown>, true);
        let _ = stream_obj.Close();
    }

    // 读取 WAV
    let bytes = std::fs::read(&wav_path).map_err(|e| format!("read wav: {e}"))?;
    let _ = std::fs::remove_file(&wav_path);

    let audio = parse_wav(&bytes)?;
    if audio.is_empty() {
        return Err("sapi produced empty audio".into());
    }
    Ok(audio)
}

#[cfg(not(windows))]
fn synthesize_impl(
    _text: &str,
    _voice: Option<&str>,
    _rate: f32,
    _volume: f32,
) -> Result<AudioOutput, String> {
    Err("sapi only supported on windows".into())
}

/// 解析 16-bit PCM WAV → AudioOutput。
pub fn parse_wav(bytes: &[u8]) -> Result<AudioOutput, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a wav".into());
    }
    let mut pos = 12usize;
    let mut sample_rate = 16000u32;
    let mut channels = 1u16;
    let mut bits = 16u16;
    let mut data: &[u8] = &[];
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body_start = pos + 8;
        if id == b"fmt " && body_start + 16 <= bytes.len() {
            channels = u16::from_le_bytes([bytes[body_start + 2], bytes[body_start + 3]]);
            sample_rate = u32::from_le_bytes([
                bytes[body_start + 4],
                bytes[body_start + 5],
                bytes[body_start + 6],
                bytes[body_start + 7],
            ]);
            bits = u16::from_le_bytes([bytes[body_start + 14], bytes[body_start + 15]]);
        } else if id == b"data" {
            let end = (body_start + size).min(bytes.len());
            data = &bytes[body_start..end];
        }
        pos = body_start + size + (size & 1);
    }
    if bits != 16 {
        return Err(format!("unsupported bits: {bits}"));
    }
    let mut pcm = Vec::with_capacity(data.len() / 2);
    let mut i = 0;
    while i + 1 < data.len() {
        pcm.push(i16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    Ok(AudioOutput {
        pcm_i16: pcm,
        sample_rate,
        channels,
        bits_per_sample: bits,
    })
}
