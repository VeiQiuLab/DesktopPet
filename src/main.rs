// 隐藏控制台窗口（GUI 程序）；调试时可注释掉。
#![windows_subsystem = "windows"]

mod agent_launcher;
mod app;
mod autostart;
mod behavior;
mod character;
mod config;
mod ipc;
mod platform;
mod presentation;
mod single_instance;

use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// 轻量 CLI 测试钩子（autostart，独立于 IPC client）。
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

    // 1) IPC client 模式（不获取 mutex，不创建第二个桌宠）
    if let Some(code) = ipc::client::run_cli(&args) {
        std::process::exit(code);
    }

    // 2) autostart 测试钩子
    if run_autostart_cli(&args) {
        return;
    }

    // 3) 正常 GUI 模式
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

    // 一键启动：静默拉起 pet-agent（单实例保护由 agent 自身负责）
    agent_launcher::launch_agent();

    let mut app = app::App::new();
    let result = app.run();

    // 退出联动：请求 agent 退出
    agent_launcher::request_agent_exit();

    if let Err(e) = result {
        config::log_error(&format!("fatal: {e}"));
        std::process::exit(1);
    }
}
