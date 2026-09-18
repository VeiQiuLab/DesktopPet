//! 配置读写：窗口位置、激活角色。
//! 目标：运行时不再依赖任何绝对路径。

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
    /// 当前激活的角色 id（对应 characters/<id>/）。
    /// 空字符串表示使用第一个可用角色。
    #[serde(default = "default_active")]
    pub active_character: String,
}

fn default_active() -> String {
    "default".to_string()
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
        }
    }
}

impl Config {
    /// 配置文件路径：exe 同目录下 desktop-pet.config.json
    pub fn path() -> PathBuf {
        let mut p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        p.pop();
        p.push("desktop-pet.config.json");
        p
    }

    /// 读取配置；不存在或解析失败则返回默认值（不 panic）。
    /// 首次运行（文件不存在）时写出默认配置。
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

    /// 保存配置；失败仅记录日志，不影响程序运行。
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

    /// 将窗口位置修正到可见区域内（当保存位置落在虚拟屏幕外时）。
    /// 保留至少 MARGIN 像素可见。
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

/// 简单日志：同时输出 stderr 与独立文件，便于诊断。
pub fn log_line(msg: &str) {
    eprintln!("[desktop-pet] {msg}");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("pet_runtime.log")
    {
        let _ = writeln!(f, "[desktop-pet] {msg}");
    }
}
