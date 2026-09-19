//! TTS 输出层（属于 pet-agent，与 DesktopPet Runtime 解耦）。
//!
//! 链路：AssistantResponse → sanitize → TtsProvider → Playback。
//! 独立 worker + 有界「最新优先」队列；可停止；失败隔离。

pub mod audio;
pub mod piper;
pub mod playback;
pub mod provider;
pub mod sanitize;
pub mod sapi;
pub mod temp;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::config::AgentConfig;
use crate::log_line;
use provider::{make as make_provider, TtsProvider};

struct Job {
    generation: u64,
    text: String,
}

struct Slot {
    job: Option<Job>,
    stopped: bool,
}

pub struct TtsController {
    enabled: bool,
    gen: Arc<AtomicU64>,
    slot: Arc<(Mutex<Slot>, Condvar)>,
    stop_flag: Arc<AtomicU64>,
}

impl TtsController {
    pub fn new(cfg: &AgentConfig) -> Self {
        let enabled = cfg.tts.enabled;
        let gen = Arc::new(AtomicU64::new(0));
        let slot = Arc::new((
            Mutex::new(Slot {
                job: None,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let stop_flag = Arc::new(AtomicU64::new(0));

        // 仅在启用时创建 provider + worker（禁用时不初始化重型资源）
        if enabled {
            let tts_cfg = cfg.tts.clone().normalized();
            let provider: Box<dyn TtsProvider> = make_provider(&tts_cfg);
            let pname = provider.name().to_string();
            let slot2 = slot.clone();
            let gen2 = gen.clone();
            let stop2 = stop_flag.clone();
            std::thread::Builder::new()
                .name("tts-worker".into())
                .spawn(move || worker_loop(provider, slot2, gen2, stop2))
                .ok();
            log_line(&format!("tts enabled (provider={pname})"));
        } else {
            log_line("tts disabled");
        }

        TtsController {
            enabled,
            gen,
            slot,
            stop_flag,
        }
    }

    /// 请求朗读。最新请求覆盖未开始的旧请求；并停止当前播放（最新优先 / 可打断）。
    pub fn speak(&mut self, text: &str) {
        if !self.enabled || text.trim().is_empty() {
            return;
        }
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        // 打断当前播放
        playback::stop();
        let (m, cv) = &*self.slot;
        if let Ok(mut s) = m.lock() {
            s.job = Some(Job {
                generation: g,
                text: text.to_string(),
            });
            s.stopped = false;
            cv.notify_one();
        }
    }

    /// 停止当前播放与待播放。
    pub fn stop(&mut self) {
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.stop_flag.fetch_add(1, Ordering::SeqCst);
        playback::stop();
        let (m, _cv) = &*self.slot;
        if let Ok(mut s) = m.lock() {
            s.job = None;
        }
    }
}

fn worker_loop(
    provider: Box<dyn TtsProvider>,
    slot: Arc<(Mutex<Slot>, Condvar)>,
    gen: Arc<AtomicU64>,
    stop_flag: Arc<AtomicU64>,
) {
    let (m, cv) = &*slot;
    let mut last_stop = stop_flag.load(Ordering::SeqCst);
    loop {
        let job = {
            let mut s = match m.lock() {
                Ok(s) => s,
                Err(_) => break,
            };
            while s.job.is_none() {
                s = match cv.wait(s) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
            s.job.take()
        };
        let job = match job {
            Some(j) => j,
            None => continue,
        };
        // 已被更新的请求取代？
        if job.generation != gen.load(Ordering::SeqCst) {
            continue;
        }
        let stop_now = stop_flag.load(Ordering::SeqCst);
        if stop_now != last_stop {
            last_stop = stop_now;
            continue;
        }

        match provider.synthesize(&job.text) {
            Ok(audio) => {
                if audio.is_empty() {
                    continue;
                }
                // 计算 envelope 并一次性发给 DesktopPet（不高频 IPC）
                if crate::config::lip_sync_enabled() {
                    let env = audio::envelope(&audio, 30.0, 1.0, 0.02);
                    if !env.is_empty() {
                        let json = pet_protocol::build_lip_sync(30.0, &env, 0);
                        let _ = crate::ipc::send_raw_pub(&json);
                    }
                }
                let wav = audio.to_wav();
                playback::play(&wav);
                // 粗略等待播放结束（可被 stop 打断）
                let secs = audio.duration_secs().clamp(0.2, 30.0);
                let step = Duration::from_millis(50);
                let mut waited = 0.0f32;
                while waited < secs {
                    if job.generation != gen.load(Ordering::SeqCst)
                        || stop_flag.load(Ordering::SeqCst) != last_stop
                    {
                        playback::stop();
                        break;
                    }
                    std::thread::sleep(step);
                    waited += 0.05;
                }
            }
            Err(e) => {
                log_line(&format!("tts error: {e}"));
            }
        }
    }
}
