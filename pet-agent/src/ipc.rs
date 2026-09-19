//! 极简 Named Pipe 客户端（与 DesktopPet 的 `\.\pipe\DesktopPetExpression_v1` 通信）。
//!
//! 不依赖 desktop-pet Runtime；只用 pet-protocol 的协议构造 + Win32 管道。

use pet_protocol::{Motion, Priority};

#[cfg(windows)]
pub fn send_expression(
    text: Option<&str>,
    motion: Option<Motion>,
    priority: Priority,
    duration_ms: Option<u64>,
) -> Result<String, String> {
    let json = pet_protocol::build_expression(text, motion, priority, duration_ms, None);
    send_raw(&json)
}

#[cfg(windows)]
#[allow(dead_code)]
pub fn send_command(command: &str) -> Result<String, String> {
    let json = pet_protocol::build_command(command, None);
    send_raw(&json)
}

#[cfg(windows)]
fn send_raw(json: &str) -> Result<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::Pipes::WaitNamedPipeW;

    let name: Vec<u16> = pet_protocol::PIPE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = match CreateFileW(
            PCWSTR(name.as_ptr()),
            0x8000_0000 | 0x4000_0000,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        ) {
            Ok(h) => h,
            Err(_) => {
                // 管道忙则短暂等待
                let _ = WaitNamedPipeW(PCWSTR(name.as_ptr()), 500);
                CreateFileW(
                    PCWSTR(name.as_ptr()),
                    0x8000_0000 | 0x4000_0000,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
                .map_err(|_| "cannot connect to DesktopPet (offline?)".to_string())?
            }
        };
        let _ = ERROR_PIPE_BUSY;

        let mut n = 0u32;
        if WriteFile(handle, Some(json.as_bytes()), Some(&mut n), None).is_err() {
            let _ = CloseHandle(handle);
            return Err("write failed".into());
        }
        let _ = FlushFileBuffers(handle);

        let mut buf = [0u8; 4096];
        let mut rn = 0u32;
        let resp = match ReadFile(handle, Some(&mut buf), Some(&mut rn), None) {
            Ok(()) if rn > 0 => String::from_utf8_lossy(&buf[..rn as usize]).to_string(),
            _ => "{\"ok\":true,\"accepted\":true}".to_string(),
        };
        let _ = CloseHandle(handle);
        Ok(resp)
    }
}

#[cfg(not(windows))]
pub fn send_expression(
    _text: Option<&str>,
    _motion: Option<Motion>,
    _priority: Priority,
    _duration_ms: Option<u64>,
) -> Result<String, String> {
    Err("named pipe only supported on windows".into())
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn send_command(_command: &str) -> Result<String, String> {
    Err("named pipe only supported on windows".into())
}
