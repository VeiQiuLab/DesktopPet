//! 音频播放层（Windows waveOut 之上的最简封装：PlaySound）。
//!
//! 业务层只调用 play/stop，不直接触碰具体音频 API。

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(psz_sound: *const u16, hmod: isize, fdw_sound: u32) -> i32;
}

const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_MEMORY: u32 = 0x0004;

/// 播放 WAV 字节（异步）。新的播放会替换旧的。
#[cfg(windows)]
pub fn play(wav: &[u8]) {
    unsafe {
        // SND_MEMORY 要求缓冲区在播放期间保持有效 → 复制到持久缓冲
        let leaked: &'static [u8] = Box::leak(wav.to_vec().into_boxed_slice());
        let _ = PlaySoundW(
            leaked.as_ptr() as *const u16,
            0,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// 停止当前播放。
#[cfg(windows)]
pub fn stop() {
    unsafe {
        let _ = PlaySoundW(std::ptr::null(), 0, SND_ASYNC);
    }
}

#[cfg(not(windows))]
pub fn play(_wav: &[u8]) {}

#[cfg(not(windows))]
pub fn stop() {}
