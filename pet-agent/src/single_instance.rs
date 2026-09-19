//! pet-agent 单实例：Named Mutex + 通知已有实例。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub const MUTEX_NAME: &str = "DesktopPet_Agent_SingleInstance_v1";

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// 尝试获取单实例锁。返回 Some(自身) / None(已有实例)。
    pub fn acquire() -> Option<Self> {
        let wide: Vec<u16> = MUTEX_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let handle = CreateMutexW(None, true, PCWSTR(wide.as_ptr())).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(SingleInstance { handle })
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// 注册一个具名消息，用于唤醒已有实例显示输入框。
pub const WAKE_MSG_NAME: &str = "DesktopPetAgent_WakeInput_v1";

pub fn wake_existing() {
    use windows::Win32::UI::WindowsAndMessaging::{
        PostMessageW, RegisterWindowMessageW, HWND_BROADCAST,
    };
    unsafe {
        let name: Vec<u16> = WAKE_MSG_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let msg = RegisterWindowMessageW(PCWSTR(name.as_ptr()));
        if msg != 0 {
            let _ = PostMessageW(
                Some(HWND_BROADCAST),
                msg,
                Default::default(),
                Default::default(),
            );
        }
    }
}

/// 广播「打开记忆管理」。
#[allow(dead_code)]
pub fn broadcast_open_memory() {
    broadcast_named("DesktopPetAgent_OpenMemory");
}

/// 广播「打开 Persona 管理」。
#[allow(dead_code)]
pub fn broadcast_open_persona() {
    broadcast_named("DesktopPetAgent_OpenPersona");
}

/// 广播「退出 agent」。
#[allow(dead_code)]
pub fn broadcast_quit() {
    broadcast_named("DesktopPetAgent_Quit");
}

pub fn named_message_id(name: &str) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
    unsafe {
        let n: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        RegisterWindowMessageW(PCWSTR(n.as_ptr()))
    }
}

#[allow(dead_code)]
fn broadcast_named(name: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST};
    unsafe {
        let msg = named_message_id(name);
        if msg != 0 {
            let _ = PostMessageW(
                Some(HWND_BROADCAST),
                msg,
                Default::default(),
                Default::default(),
            );
        }
    }
}

/// 取得唤醒消息 ID。
pub fn wake_message_id() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
    unsafe {
        let name: Vec<u16> = WAKE_MSG_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        RegisterWindowMessageW(PCWSTR(name.as_ptr()))
    }
}
