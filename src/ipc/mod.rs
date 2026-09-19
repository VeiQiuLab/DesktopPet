//! 本地 IPC：Named Pipe 外部控制入口。
//!
//! 边界：
//! - IPC worker 线程只负责接收 / 解码 / 校验 / 入队（有界 channel）。
//! - HWND / Presentation / Cubism / Behavior 只在主线程操作。
//! - 不使用 HTTP / 网络 / 数据库 / 管理员权限。

pub mod client;
pub mod protocol;
pub mod server;
