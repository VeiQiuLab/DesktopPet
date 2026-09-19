//! Named Pipe server：worker 线程接收 → 校验 → 有界 channel → 主线程。
//!
//! - 线程边界：worker 不触碰 HWND / Cubism / Presentation。
//! - 有界 channel：队列满返回 queue_full，不无限增长。
//! - 关闭：用 stop flag + 主动连接唤醒阻塞的 ConnectNamedPipe。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::ipc::status::SharedStatus;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_BROKEN_PIPE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::config::{log_debug, log_error};

/// 管道路径（版本化，来自共享协议 crate）。
pub const PIPE_NAME: &str = pet_protocol::PIPE_NAME;

/// 队列容量（主线程与 IPC worker 之间的有界通道）。
pub const QUEUE_CAPACITY: usize = 32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// SDDL：只允许 Owner（当前用户）+ Local System 完全访问。
/// 避免默认 ACL 可能授予 Everyone 读权限。
const PIPE_SDDL: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

/// 构建 Named Pipe 的 SECURITY_ATTRIBUTES（限制到当前用户 + SYSTEM）。
///
/// 返回 (SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR 句柄)。调用方用完需
/// 通过 `free_security_descriptor` 释放描述符。
unsafe fn build_pipe_security() -> Option<(
    windows::Win32::Security::SECURITY_ATTRIBUTES,
    windows::Win32::Security::PSECURITY_DESCRIPTOR,
)> {
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    let sddl = wide(PIPE_SDDL);
    let mut psd = PSECURITY_DESCRIPTOR::default();
    let ok = ConvertStringSecurityDescriptorToSecurityDescriptorW(
        PCWSTR(sddl.as_ptr()),
        SDDL_REVISION_1,
        &mut psd,
        None,
    );
    if ok.is_err() || psd.is_invalid() {
        return None;
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: psd.0,
        bInheritHandle: windows::core::BOOL(0),
    };
    Some((sa, psd))
}

unsafe fn free_security_descriptor(psd: windows::Win32::Security::PSECURITY_DESCRIPTOR) {
    if !psd.is_invalid() {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        let _ = LocalFree(Some(HLOCAL(psd.0)));
    }
}

/// IPC 服务端句柄。Drop 时停止 worker。
pub struct IpcServer {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl IpcServer {
    /// 启动 server。返回 (IpcServer, Receiver, SharedStatus)。
    pub fn start() -> (
        Self,
        Receiver<crate::ipc::protocol::ValidatedRequest>,
        Arc<SharedStatus>,
    ) {
        let (tx, rx) = sync_channel::<crate::ipc::protocol::ValidatedRequest>(QUEUE_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let status = Arc::new(SharedStatus::new());
        let status2 = status.clone();

        let join = std::thread::Builder::new()
            .name("ipc-worker".into())
            .spawn(move || worker_loop(tx, stop2, status2))
            .ok();

        (IpcServer { stop, join }, rx, status)
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 唤醒阻塞的 ConnectNamedPipe：客户端连一下再断开
        let _ = send_request("{\"version\":1,\"type\":\"command\",\"command\":\"__stop\"}");
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// worker 主循环。
fn worker_loop(
    tx: SyncSender<crate::ipc::protocol::ValidatedRequest>,
    stop: Arc<AtomicBool>,
    status: Arc<SharedStatus>,
) {
    unsafe {
        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let name = wide(PIPE_NAME);
            // 限制 ACL 到当前用户 + SYSTEM
            let (sa_opt, psd_opt) = match build_pipe_security() {
                Some((sa, psd)) => (Some(sa), Some(psd)),
                None => (None, None),
            };
            let handle = CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                sa_opt.as_ref().map(|sa| sa as *const _),
            );
            if let Some(psd) = psd_opt {
                free_security_descriptor(psd);
            }
            if handle == windows::Win32::Foundation::INVALID_HANDLE_VALUE {
                log_error("CreateNamedPipeW failed");
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }

            // 阻塞等待客户端连接
            let _ = ConnectNamedPipe(handle, None);
            let err = GetLastError();
            // ERROR_PIPE_CONNECTED(535) 也算已连接
            if err.0 != 0 && err.0 != 535 && stop.load(Ordering::SeqCst) {
                let _ = CloseHandle(handle);
                break;
            }

            handle_connection(handle, &tx, &stop, &status);

            let _ = DisconnectNamedPipe(handle);
            let _ = CloseHandle(handle);
        }
    }
    log_debug("ipc worker exited");
}

/// 处理一次连接：读一条消息 → 校验 → 入队 → 写响应。
unsafe fn handle_connection(
    handle: HANDLE,
    tx: &SyncSender<crate::ipc::protocol::ValidatedRequest>,
    stop: &AtomicBool,
    status: &SharedStatus,
) {
    // 读取：单条消息，最大 MAX_MESSAGE_BYTES
    let mut buf = vec![0u8; crate::ipc::protocol::MAX_MESSAGE_BYTES + 1];
    let mut total = 0usize;
    loop {
        if total >= buf.len() {
            break;
        }
        let mut n = 0u32;
        match ReadFile(handle, Some(&mut buf[total..]), Some(&mut n), None) {
            Ok(()) => {
                if n == 0 {
                    break;
                }
                total += n as usize;
                // 简化协议：一条消息一次发送；客户端发完即等待响应。
                // 若能解析出完整 JSON，则停止读取。
                if serde_json::from_slice::<serde_json::Value>(&buf[..total]).is_ok() {
                    break;
                }
            }
            Err(e) => {
                if e.code().0 as u32 == ERROR_BROKEN_PIPE.0 {
                    // 客户端断开
                }
                break;
            }
        }
    }

    if total == 0 {
        return;
    }
    if total > crate::ipc::protocol::MAX_MESSAGE_BYTES {
        let resp = crate::ipc::protocol::Response::err(None, "message too large");
        write_response(handle, &resp);
        return;
    }

    let text = match std::str::from_utf8(&buf[..total]) {
        Ok(s) => s,
        Err(_) => {
            let resp = crate::ipc::protocol::Response::err(None, "invalid utf-8");
            write_response(handle, &resp);
            return;
        }
    };

    // __stop 是内部唤醒消息，不入队
    if text.contains("__stop") {
        return;
    }

    match crate::ipc::protocol::parse_request(text) {
        Ok(req) => {
            // Query 是只读请求：worker 直接从共享快照应答，不入队。
            if let crate::ipc::protocol::ValidatedRequest::Query { id, query } = &req {
                let resp = if query == "status" {
                    let snap = status.snapshot();
                    let json = serde_json::to_string(&snap).unwrap_or_default();
                    // 用 ok + 内嵌 status 字段
                    format!(
                        "{{\"ok\":true,\"id\":{},\"status\":{json}}}",
                        json_opt_id(id)
                    )
                } else {
                    crate::ipc::protocol::Response::err(id.clone(), "unknown query").to_json()
                };
                write_raw(handle, &resp);
                return;
            }
            let id = match &req {
                crate::ipc::protocol::ValidatedRequest::Expression(e) => e.id.clone(),
                crate::ipc::protocol::ValidatedRequest::Command { id, .. } => id.clone(),
                crate::ipc::protocol::ValidatedRequest::Query { id, .. } => id.clone(),
            };
            // 有界队列：满则快速失败
            match tx.try_send(req) {
                Ok(()) => {
                    let resp = crate::ipc::protocol::Response::ok(id);
                    write_response(handle, &resp);
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    let resp = crate::ipc::protocol::Response::err(id, "queue_full");
                    write_response(handle, &resp);
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    let resp = crate::ipc::protocol::Response::err(id, "server_shutting_down");
                    write_response(handle, &resp);
                }
            }
        }
        Err(e) => {
            let resp = crate::ipc::protocol::Response::err(None, e.to_string());
            write_response(handle, &resp);
        }
    }
    let _ = stop;
}

unsafe fn write_response(handle: HANDLE, resp: &crate::ipc::protocol::Response) {
    write_raw(handle, &resp.to_json());
}

unsafe fn write_raw(handle: HANDLE, json: &str) {
    let mut n = 0u32;
    let _ = WriteFile(handle, Some(json.as_bytes()), Some(&mut n), None);
    let _ = FlushFileBuffers(handle);
}

fn json_opt_id(id: &Option<String>) -> String {
    match id {
        Some(s) => serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    }
}

/// 客户端：发送一条 JSON 请求并读取响应（供 CLI 使用）。
pub fn send_request(json: &str) -> Result<String, String> {
    unsafe {
        let name = wide(PIPE_NAME);
        // 等待管道可用（最多 2 秒）
        for _ in 0..20 {
            match CreateFileW(
                PCWSTR(name.as_ptr()),
                0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            ) {
                Ok(handle) => {
                    let mut n = 0u32;
                    if WriteFile(handle, Some(json.as_bytes()), Some(&mut n), None).is_err() {
                        let _ = CloseHandle(handle);
                        return Err("write failed".into());
                    }
                    let _ = FlushFileBuffers(handle);
                    // 读响应
                    let mut buf = [0u8; 4096];
                    let mut rn = 0u32;
                    let resp = match ReadFile(handle, Some(&mut buf), Some(&mut rn), None) {
                        Ok(()) if rn > 0 => {
                            String::from_utf8_lossy(&buf[..rn as usize]).to_string()
                        }
                        _ => "{\"ok\":true,\"accepted\":true}".to_string(),
                    };
                    let _ = CloseHandle(handle);
                    return Ok(resp);
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
        Err("cannot connect to DesktopPet (is it running?)".into())
    }
}
