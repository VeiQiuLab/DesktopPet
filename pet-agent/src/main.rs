//! pet-agent：DesktopPet 的 AI 桥接进程（独立于 desktop-pet.exe）。
//!
//! 流程：User Input → Provider → AssistantResponse → ExpressionMapper
//!       → pet-protocol → Named Pipe → DesktopPet → Bubble/Behavior。
//!
//! 本进程不接触 HWND / Cubism / D3D；不修改桌宠配置；不绕过 IPC。

mod autostart;
mod config;
mod context;
mod ipc;
mod mapper;
mod provider;
#[cfg(windows)]
mod single_instance;
mod tts;
#[cfg(windows)]
mod ui;
mod worker;

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
        "ui" => {
            #[cfg(windows)]
            {
                // 单实例：已有实例则唤醒并退出
                let _instance = match single_instance::SingleInstance::acquire() {
                    Some(i) => i,
                    None => {
                        single_instance::wake_existing();
                        eprintln!("[pet-agent] another instance is running, waking it");
                        std::process::exit(0);
                    }
                };
                ui::run_ui(provider_override.as_deref());
            }
            #[cfg(not(windows))]
            eprintln!("ui mode only supported on windows");
        }
        "tts-voices" => run_tts_voices(),
        "envelope-test" => run_envelope_test(),
        "provider-test" => run_provider_test(),
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
    println!("  pet-agent ui [--provider ...]   # 常驻输入框 + 托盘 + 全局快捷键");
    println!("  pet-agent tts-voices            # 列出系统可用 SAPI voice");
    println!("  pet-agent provider-test         # 用配置的 Provider 发一次极短请求");
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

/// 验证 envelope 算法（内部生成 tone，不播放）。
fn run_envelope_test() {
    use tts::audio::{envelope, AudioOutput};
    // 生成 1 秒 440Hz 正弦（含静音段）
    let sr = 16000u32;
    let mut pcm = Vec::with_capacity(sr as usize);
    for i in 0..sr {
        let t = i as f32 / sr as f32;
        let v = if t < 0.3 {
            0.0
        } else if t < 0.7 {
            (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.8
        } else {
            0.0
        };
        pcm.push((v * 32767.0) as i16);
    }
    let audio = AudioOutput {
        pcm_i16: pcm,
        sample_rate: sr,
        channels: 1,
        bits_per_sample: 16,
    };
    let env = envelope(&audio, 30.0, 1.0, 0.02);
    let n = env.len();
    let max = env.iter().cloned().fold(0.0f32, f32::max);
    let all_finite = env.iter().all(|v| v.is_finite());
    let all_in_range = env.iter().all(|v| *v >= 0.0 && *v <= 1.0);
    // 前段静音应接近 0
    let head_quiet = env.first().map(|v| *v < 0.05).unwrap_or(false);
    // 中段应有较大值
    let mid_loud = env.iter().skip(n / 3).take(n / 3).any(|v| *v > 0.3);
    println!("samples={n} max={max:.3} finite={all_finite} in_range={all_in_range} head_quiet={head_quiet} mid_loud={mid_loud}");
}

/// 列出系统可用 SAPI voice。
fn run_tts_voices() {
    let voices = tts::sapi::SapiProvider::list_voices();
    if voices.is_empty() {
        println!("no SAPI voices found");
    } else {
        println!("SAPI voices:");
        for (name, desc) in voices {
            println!("  {name}  ({desc})");
        }
    }
}

/// 用配置的 Provider 发一次极短请求（不发送到 DesktopPet）。
fn run_provider_test() {
    let cfg = AgentConfig::load();
    let provider = build_provider(&cfg, None);
    let msgs = vec![context::ChatMessage {
        role: "system",
        content: cfg.system_prompt.clone(),
    }];
    let started = std::time::Instant::now();
    match provider.generate(&msgs, "只回复：测试成功") {
        Ok(r) => {
            println!("provider: {}", provider.name());
            println!("model: {}", cfg.model);
            println!("latency: {}ms", started.elapsed().as_millis());
            println!("response: {r}");
        }
        Err(e) => {
            println!("provider unavailable: {e}");
        }
    }
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

/// Agent 日志（写入 exe 同目录 pet-agent.log，默认不记录对话内容）。
pub fn log_line(msg: &str) {
    eprintln!("[pet-agent] {msg}");
    use std::io::Write;
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    p.push("pet-agent.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = writeln!(f, "[pet-agent] {msg}");
    }
}
