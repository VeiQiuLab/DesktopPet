//! pet-agent：DesktopPet 的 AI 桥接进程（独立于 desktop-pet.exe）。
//!
//! 流程：User Input → Provider → AssistantResponse → ExpressionMapper
//!       → pet-protocol → Named Pipe → DesktopPet → Bubble/Behavior。
//!
//! 本进程不接触 HWND / Cubism / D3D；不修改桌宠配置；不绕过 IPC。

mod config;
mod context;
mod ipc;
mod mapper;
mod provider;

use std::io::{self, BufRead, Write};

use config::AgentConfig;
use provider::Provider;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("chat");

    // --provider 覆盖
    let provider_override = args
        .iter()
        .position(|a| a == "--provider")
        .and_then(|i| args.get(i + 1).cloned());

    match sub {
        "chat" => run_chat(provider_override.as_deref()),
        "once" => {
            let text = args.get(2).cloned().unwrap_or_default();
            run_once(&text, provider_override.as_deref());
        }
        "help" | "--help" | "-h" => print_help(),
        _ => {
            eprintln!("unknown subcommand: {sub}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("pet-agent — DesktopPet AI bridge");
    println!();
    println!("Usage:");
    println!("  pet-agent chat [--provider mock|openai_compatible]");
    println!("  pet-agent once \"你好\" [--provider ...]");
}

fn build_provider(cfg: &AgentConfig, override_name: Option<&str>) -> Box<dyn Provider> {
    let name = override_name.unwrap_or(&cfg.provider);
    provider::make(name, cfg)
}

fn run_chat(provider_override: Option<&str>) {
    let cfg = AgentConfig::load();
    let mut ctx = context::Conversation::new(&cfg);
    let provider = build_provider(&cfg, provider_override);

    log_line(&format!("pet-agent started (provider={})", provider.name()));

    println!("pet-agent chat. Type your message. Ctrl-C / 'exit' to quit.");
    let stdin = io::stdin();
    loop {
        print!("You > ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("input error: {e}");
                break;
            }
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if text == "exit" || text == "quit" {
            break;
        }
        handle_turn(&*provider, &cfg, &mut ctx, text);
    }
    log_line("pet-agent exited");
}

fn run_once(text: &str, provider_override: Option<&str>) {
    let cfg = AgentConfig::load();
    let mut ctx = context::Conversation::new(&cfg);
    let provider = build_provider(&cfg, provider_override);
    handle_turn(&*provider, &cfg, &mut ctx, text);
}

fn handle_turn(
    provider: &dyn Provider,
    cfg: &AgentConfig,
    ctx: &mut context::Conversation,
    user_text: &str,
) {
    let started = std::time::Instant::now();
    let reply = match provider.generate(&ctx.messages(), user_text) {
        Ok(r) => r,
        Err(e) => {
            log_line(&format!("provider error: {e}"));
            eprintln!("[provider error] {e}");
            // 可选：向桌宠发送 "……"，但不暴露错误栈
            let _ = ipc::send_expression(Some("……"), None, pet_protocol::Priority::Normal, None);
            return;
        }
    };
    let elapsed = started.elapsed().as_millis();
    log_line(&format!(
        "provider replied in {elapsed}ms ({} chars)",
        reply.chars().count()
    ));

    // Console：完整回复
    println!("Pet > {reply}");

    ctx.push_user(user_text);
    ctx.push_assistant(&reply);

    // Bubble：短版本（截断）
    let bubble_text = truncate_for_bubble(&reply, cfg.bubble_max_chars);

    // ExpressionMapper：由本地规则决定是否附带动作
    let mapped = mapper::map(&bubble_text);

    match ipc::send_expression(Some(&mapped.text), mapped.motion, mapped.priority, None) {
        Ok(resp) => log_line(&format!("ipc ok: {resp}")),
        Err(e) => {
            log_line(&format!("ipc error: {e}"));
            eprintln!("[desktop-pet offline?] {e}");
        }
    }
}

/// 截断为适合气泡的短版本。
fn truncate_for_bubble(text: &str, max_chars: usize) -> String {
    let cleaned = text.replace('\r', " ").trim().to_string();
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }
    let mut out: String = cleaned.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Agent 日志（写入 pet-agent.log，默认不记录对话内容）。
pub fn log_line(msg: &str) {
    eprintln!("[pet-agent] {msg}");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("pet-agent.log")
    {
        let _ = writeln!(f, "[pet-agent] {msg}");
    }
}
