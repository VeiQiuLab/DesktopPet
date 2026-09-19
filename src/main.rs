//! DesktopPet — Live2D 桌宠 Runtime。
//!
//! 单进程 GUI：双击本 exe 即出现桌宠，无子进程、无控制台。

#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod behavior;
mod character;
mod config;
mod platform;
mod presentation;
mod single_instance;

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// 轻量 CLI 测试钩子（autostart，独立于 GUI）。
fn run_autostart_cli(args: &[String]) -> bool {
    if args.len() < 2 {
        return false;
    }
    let out: String;
    match args[1].as_str() {
        "autostart-status" => out = format!("autostart={}", autostart::is_enabled()),
        "autostart-enable" => {
            let _ = autostart::enable();
            out = format!("autostart={}", autostart::is_enabled());
        }
        "autostart-disable" => {
            let _ = autostart::disable();
            out = format!("autostart={}", autostart::is_enabled());
        }
        "autostart-toggle" => out = format!("autostart={}", autostart::toggle()),
        _ => return false,
    }
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    p.push("cli_out.txt");
    let _ = std::fs::write(&p, &out);
    true
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // autostart 测试钩子
    if run_autostart_cli(&args) {
        return;
    }

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let _instance = match single_instance::SingleInstance::acquire("DesktopPet_SingleInstance_v1") {
        Some(i) => i,
        None => {
            config::log_line("another instance is already running, exiting");
            std::process::exit(0);
        }
    };

    config::reset_log_file();

    let mut app = app::App::new();
    if let Err(e) = app.run() {
        config::log_error(&format!("fatal: {e}"));
        std::process::exit(1);
    }
}
