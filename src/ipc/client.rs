//! 轻量 CLI 客户端：desktop-pet.exe <subcommand> ...
//!
//! 仅作为 IPC client：连接正在运行的 DesktopPet，发送请求，打印结果。
//! 不获取主实例 mutex，不创建第二个桌宠。

use super::server::send_request;

fn make_req(json: &str) -> Result<String, String> {
    send_request(json)
}

/// 处理 CLI 参数。返回 Some(exit_code) 表示已作为 CLI client 运行完；None 表示非 CLI 模式。
pub fn run_cli(args: &[String]) -> Option<i32> {
    if args.len() < 2 {
        return None;
    }
    let sub = args[1].as_str();

    let json: String = match sub {
        "send" => {
            // send "text" [--motion X] [--priority Y] [--duration N]
            let text = args.get(2).cloned().unwrap_or_default();
            let mut motion = None;
            let mut priority = None;
            let mut duration = None;
            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--motion" => {
                        motion = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--priority" => {
                        priority = args.get(i + 1).cloned();
                        i += 2;
                    }
                    "--duration" => {
                        duration = args.get(i + 1).and_then(|s| s.parse::<u32>().ok());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            let mut obj = serde_json::json!({
                "version": 1,
                "type": "expression",
                "text": text,
            });
            if let Some(m) = motion {
                obj["motion"] = serde_json::Value::String(m);
            }
            if let Some(p) = priority {
                obj["priority"] = serde_json::Value::String(p);
            }
            if let Some(d) = duration {
                obj["duration_ms"] = serde_json::Value::Number(d.into());
            }
            obj.to_string()
        }
        "motion" => {
            let m = args.get(2).cloned().unwrap_or_default();
            serde_json::json!({
                "version": 1, "type": "expression", "motion": m
            })
            .to_string()
        }
        "hide" => serde_json::json!({"version":1,"type":"command","command":"hide"}).to_string(),
        "show" => serde_json::json!({"version":1,"type":"command","command":"show"}).to_string(),
        "dismiss" => {
            serde_json::json!({"version":1,"type":"command","command":"dismiss_bubble"}).to_string()
        }
        "reset" => {
            serde_json::json!({"version":1,"type":"command","command":"reset_position"}).to_string()
        }
        "character" => {
            let id = args.get(2).cloned().unwrap_or_default();
            serde_json::json!({
                "version":1,"type":"command","command":"character","character":id
            })
            .to_string()
        }
        "status" | "query" => pet_protocol::build_query("status"),
        _ => return None,
    };

    let result = make_req(&json);
    let output = match result {
        Ok(resp) => resp,
        Err(e) => format!("{{\"ok\":false,\"error\":\"{e}\"}}"),
    };
    write_output(&output);
    Some(0)
}

/// 输出 CLI 结果：优先写父进程控制台，同时写 cli_out.txt（GUI 子系统无控制台时的兜底）。
fn write_output(text: &str) {
    // cli_out.txt
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    p.push("cli_out.txt");
    let _ = std::fs::write(&p, text);

    // 尝试附着父控制台
    unsafe {
        use windows::Win32::Storage::FileSystem::WriteFile;
        use windows::Win32::System::Console::{
            AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
        };
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            if let Ok(h) = GetStdHandle(STD_OUTPUT_HANDLE) {
                let msg = format!("{text}\r\n");
                let wide: Vec<u16> = msg.encode_utf16().collect();
                let bytes: &[u8] =
                    std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                let mut n = 0u32;
                let _ = WriteFile(h, Some(bytes), Some(&mut n), None);
            }
        }
    }
}
