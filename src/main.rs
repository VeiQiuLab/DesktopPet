// 隐藏控制台窗口（GUI 程序）；调试时可注释掉。
#![windows_subsystem = "windows"]

mod app;
mod behavior;
mod character;
mod config;
mod platform;

fn main() {
    let mut app = app::App::new();
    if let Err(e) = app.run() {
        config::log_line(&format!("fatal: {e}"));
        std::process::exit(1);
    }
}
