//! AudioOutput：Provider 合成的音频（统一结构）。
//!
//! 所有 Provider 都返回它，统一经 Playback 播放；不各自绕过。

#[derive(Debug, Clone)]
pub struct AudioOutput {
    /// 16-bit PCM 采样（mono），已按 sample_rate 归一。
    pub pcm_i16: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

impl AudioOutput {
    pub fn empty() -> Self {
        AudioOutput {
            pcm_i16: Vec::new(),
            sample_rate: 16000,
            channels: 1,
            bits_per_sample: 16,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pcm_i16.is_empty()
    }

    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }
        self.pcm_i16.len() as f32 / (self.sample_rate as f32 * self.channels as f32)
    }

    /// 打包为 16-bit PCM WAV 字节（供 PlaySound 使用）。
    pub fn to_wav(&self) -> Vec<u8> {
        let channels = self.channels.max(1);
        let bits = self.bits_per_sample.max(8);
        let bytes_per_sample = (bits / 8) as u32;
        let data_size = self.pcm_i16.len() as u32 * bytes_per_sample;
        let byte_rate = self.sample_rate * channels as u32 * bytes_per_sample;
        let block_align = channels * (bits / 8);

        let mut out = Vec::with_capacity(44 + data_size as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_size).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_size.to_le_bytes());
        for s in &self.pcm_i16 {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }
}

/// 从 AudioOutput 计算归一化嘴型 envelope（约 sample_hz Hz）。
pub fn envelope(audio: &AudioOutput, sample_hz: f32, gain: f32, noise_floor: f32) -> Vec<f32> {
    let sr = audio.sample_rate.max(1) as f32;
    let ch = audio.channels.max(1) as f32;
    let frame_per_win = (sr / sample_hz.clamp(1.0, 120.0)).max(1.0) as usize;
    let total_frames = (audio.pcm_i16.len() as f32 / ch) as usize;
    let mut out = Vec::new();
    let mut i = 0usize;
    // 计算总体峰值用于归一化（从 0 开始，否则峰值<1时归一化失效）
    let mut peak = 0.0f32;
    for s in &audio.pcm_i16 {
        let v = (*s as f32 / 32768.0).abs();
        if v > peak {
            peak = v;
        }
    }
    while i < total_frames {
        let end = (i + frame_per_win).min(total_frames);
        let mut sum_sq = 0.0f32;
        let mut n = 0usize;
        for f in i..end {
            // 取该帧第一个声道
            let s = audio.pcm_i16[f * ch as usize];
            let v = s as f32 / 32768.0;
            sum_sq += v * v;
            n += 1;
        }
        let rms = if n > 0 {
            (sum_sq / n as f32).sqrt()
        } else {
            0.0
        };
        // 归一化 + gain + noise floor
        let mut v = (rms / peak.max(1e-4)) * gain;
        if v < noise_floor {
            v = 0.0;
        }
        out.push(v.clamp(0.0, 1.0));
        i = end;
    }
    // 限制长度（约 30s @ sample_hz）
    out.truncate((sample_hz * 30.0) as usize);
    out
}
