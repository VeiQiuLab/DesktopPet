//! 单实例保护：命名互斥体（Named Mutex）。
//!
//! 第一个实例创建并持有互斥体；后续实例检测到已存在则放弃启动。

use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// 尝试获取单实例锁。若已有实例运行，返回 None。
    pub fn acquire(name: &str) -> Option<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let handle = CreateMutexW(None, true, windows::core::PCWSTR(wide.as_ptr())).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                // 另一个实例已持有；关闭自己刚拿到的句柄并放弃。
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
