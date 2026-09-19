//! 统一临时音频目录：%TEMP%\DesktopPet\tts\

use std::path::PathBuf;

pub fn tts_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("DesktopPet");
    p.push("tts");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// 生成唯一临时文件路径（基于时间 + 计数，不基于用户文本）。
pub fn unique_path(ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    tts_dir().join(format!("tts_{ms}_{n}.{ext}"))
}

/// 启动时清理过期临时文件（> 1 小时）。
pub fn cleanup() {
    let dir = tts_dir();
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
