// 隐藏控制台窗口（GUI 程序）；调试时可注释掉。
#![windows_subsystem = "windows"]

mod app;
mod autostart;
mod behavior;
mod character;
mod config;
mod platform;
mod single_instance;

/// 轻量 CLI 测试钩子（不影响正常 GUI 运行）：
/// 仅在带参数时触发；结果写入 exe 同目录的 cli_out.txt。
fn run_cli_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return false;
    }
    let mut out = String::new();
    match args[1].as_str() {
        "autostart-status" => {
            out = format!("autostart={}", autostart::is_enabled());
        }
        "autostart-enable" => {
            let _ = autostart::enable();
            out = format!("autostart={}", autostart::is_enabled());
        }
        "autostart-disable" => {
            let _ = autostart::disable();
            out = format!("autostart={}", autostart::is_enabled());
        }
        "autostart-toggle" => {
            let now = autostart::toggle();
            out = format!("autostart={now}");
        }
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
