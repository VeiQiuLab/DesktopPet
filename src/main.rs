// 隐藏控制台窗口（GUI 程序）；调试时可注释掉。
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

/// 轻量 CLI 测试钩子（不影响正常 GUI 运行）。
fn run_cli_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
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
    if run_cli_if_requested() {
        return;
    }

    // Per-Monitor DPI Awareness V2（在创建任何窗口前设置）。
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
