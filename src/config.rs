//! 配置读写：窗口位置、激活角色、可见性、开机启动。
//! 向后兼容：字段缺失时使用默认值。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub window: WindowConfig,
    #[serde(default)]
    pub character: CharacterConfig,
    /// 是否随 Windows 启动（HKCU Run）。
    #[serde(default)]
    pub auto_start: bool,
    /// 上次退出时窗口是否可见。用于下次启动恢复（默认 true）。
    #[serde(default = "default_visible")]
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterConfig {
    #[serde(default = "default_active")]
    pub active_character: String,
}

fn default_active() -> String {
    "default".to_string()
}
fn default_visible() -> bool {
    true
}

impl Default for CharacterConfig {
    fn default() -> Self {
        CharacterConfig {
            active_character: default_active(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window: WindowConfig {
                x: 200,
                y: 200,
                width: 640,
                height: 640,
            },
            character: CharacterConfig::default(),
            auto_start: false,
            visible: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let mut p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        p.pop();
        p.push("desktop-pet.config.json");
        p
    }

    pub fn load() -> Self {
        let path = Self::path();
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(cfg) => {
                    log_line(&format!("config loaded: {}", path.display()));
                    cfg
                }
                Err(e) => {
                    log_line(&format!("config parse error ({e}), using default"));
                    Config::default()
                }
            },
            Err(_) => {
                log_line("config not found, writing default");
                let mut c = Config::default();
                c.clamp_to_screen();
                c.save();
                c
            }
        };
        cfg.clamp_to_screen();
        cfg
    }

    pub fn save(&self) {
        let path = Self::path();
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    log_line(&format!("config save failed: {e}"));
                }
            }
            Err(e) => log_line(&format!("config serialize failed: {e}")),
        }
    }

    fn clamp_to_screen(&mut self) {
        const MARGIN: i32 = 100;
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if vw <= 0 || vh <= 0 {
                return;
            }
            let w = self.window.width;
            let h = self.window.height;
            let min_x = vx - w + MARGIN;
            let max_x = vx + vw - MARGIN;
            let min_y = vy - h + MARGIN;
            let max_y = vy + vh - MARGIN;
            let nx = self.window.x.clamp(min_x, max_x);
            let ny = self.window.y.clamp(min_y, max_y);
            if nx != self.window.x || ny != self.window.y {
                log_line(&format!(
                    "window position clamped: ({},{}) -> ({},{})",
                    self.window.x, self.window.y, nx, ny
                ));
                self.window.x = nx;
                self.window.y = ny;
            }
        }
    }
}

/// 日志级别：Release 下只记录错误和关键信息，Debug 记录全部。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LogLevel {
    Debug,
    Info,
    Error,
}

/// 当前日志级别（编译期决定：Debug 构建记录全部，Release 只记录 Info/Error）。
#[inline]
pub fn log_enabled(level: LogLevel) -> bool {
    if cfg!(debug_assertions) {
        true
    } else {
        !matches!(level, LogLevel::Debug)
    }
}

/// 简单日志：同时输出 stderr 与独立文件。
/// 每次进程启动时截断日志文件，避免长期常驻导致日志无限增长。
pub fn log_line(msg: &str) {
    log_at(LogLevel::Info, msg);
}

/// Debug 级别日志（Release 下不输出）。
#[allow(dead_code)]
pub fn log_debug(msg: &str) {
    log_at(LogLevel::Debug, msg);
}

/// Error 级别日志（总是输出）。
pub fn log_error(msg: &str) {
    log_at(LogLevel::Error, msg);
}

fn log_at(level: LogLevel, msg: &str) {
    if !log_enabled(level) {
        return;
    }
    let tag = match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Error => "error",
    };
    eprintln!("[desktop-pet/{tag}] {msg}");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path())
    {
        let _ = writeln!(f, "[desktop-pet/{tag}] {msg}");
    }
}

/// 日志文件路径（exe 同目录 pet_runtime.log）。
fn log_file_path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    p.pop();
    p.push("pet_runtime.log");
    p
}

/// 启动时截断日志文件（避免无限增长）。
pub fn reset_log_file() {
    let p = log_file_path();
    let _ = std::fs::write(&p, "");
}
