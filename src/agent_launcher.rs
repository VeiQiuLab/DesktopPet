//! 启动/结束 pet-agent 子进程（静默，无控制台）。

use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::config::log_line;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// 查找 pet-agent.exe：同目录优先，其次项目布局回退。
fn agent_exe() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();

    // 1) 同目录
    let same = dir.join("pet-agent.exe");
    if same.is_file() {
        return Some(same);
    }
    // 2) 项目布局：<root>/target/<profile>/ -> <root>/pet-agent/target/<profile>/
    let profile = dir.file_name()?.to_string_lossy().to_string();
    let mut root = dir.clone();
    root.pop(); // target
    root.pop(); // 项目根
    let cand = root
        .join("pet-agent")
        .join("target")
        .join(&profile)
        .join("pet-agent.exe");
    if cand.is_file() {
        return Some(cand);
    }
    None
}

/// 启动 pet-agent ui（若尚未运行）。静默、不阻塞。
pub fn launch_agent() {
    let exe = match agent_exe() {
        Some(e) => e,
        None => {
            log_line("pet-agent.exe not found, skipping launch");
            return;
        }
    };
    // 先探测是否已有实例（通过单实例互斥体名）
    match Command::new(&exe)
        .arg("ui")
        .arg("--parent-pid")
        .arg(std::process::id().to_string())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
    {
        Ok(_) => log_line("pet-agent launched (silent)"),
        Err(e) => log_line(&format!("failed to launch pet-agent: {e}")),
    }
}

/// 请求 pet-agent 退出（广播具名消息）。
pub fn request_agent_exit() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        PostMessageW, RegisterWindowMessageW, HWND_BROADCAST,
    };
    let name: Vec<u16> = "DesktopPetAgent_Quit"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let msg = RegisterWindowMessageW(PCWSTR(name.as_ptr()));
        if msg != 0 {
            let _ = PostMessageW(
                Some(HWND_BROADCAST),
                msg,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}
