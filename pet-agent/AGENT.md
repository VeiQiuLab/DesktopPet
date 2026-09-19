# pet-agent — DesktopPet AI Bridge

独立进程，把用户文本经 AI Provider 转换为桌宠表达，通过 Named Pipe 发给 DesktopPet。

## 架构

```
User Text → pet-agent → Provider(LLM) → ExpressionMapper → Named Pipe → DesktopPet → Bubble/Behavior
```

## 构建与运行

```powershell
cd pet-agent
cargo build --release

# 交互模式
.\target\debug\pet-agent.exe chat

# 单条
.\target\debug\pet-agent.exe once "你好"
.\target\debug\pet-agent.exe once "你好" --provider mock
```

## Provider

- `mock`：本地模板回复，用于验证链路。
- `openai_compatible`：连接任意 OpenAI-compatible endpoint（llama-server / 本地 Qwen / 云）。

## 配置 pet-agent.config.json

```json
{
  "provider": "mock",
  "base_url": "http://127.0.0.1:8080/v1",
  "model": "your-model",
  "api_key": null,
  "timeout_secs": 60,
  "system_prompt": "你是桌面角色的对话层……",
  "history_limit": 12,
  "bubble_max_chars": 80,
  "log_conversation": false
}
```

不自动下载模型、不启动/杀死用户的模型服务、不扫描全机。

## UI 模式（第八阶段）

```powershell
pet-agent.exe ui      # 常驻：原生输入框 + 托盘 + 全局快捷键
```

- Enter 发送 / Shift+Enter 换行 / Esc 隐藏；`Ctrl+Alt+Space` 唤起。
- 输入框优先出现在桌宠附近（经 IPC `query_status`）。
- Agent 托盘：打开输入框 / 清空对话 / 开机启动 / 退出（键名 `DesktopPetAgent`）。
- 单实例：`DesktopPet_Agent_SingleInstance_v1`；第二实例唤醒已有实例后退出。
- AI 请求在 worker 线程，UI 不冻结；可「停止」取消（结果丢弃）。
- Provider 离线不退出；显示 Offline/Error，详细错误写日志。

## TTS 输出层（第九阶段）

- 配置 `tts.enabled=true` 后，AI 回复会经 `sanitize → TtsProvider → Playback` 朗读。
- 当前 Provider：`null`（不发声）、`mock`（静音，验证链路）、`sapi`（Windows 本地语音，无需 Key）。
- 子命令：`pet-agent tts-voices`（列出 voice）、`pet-agent envelope-test`（验证 envelope）、`pet-agent provider-test`。
- Lip Sync：`tts.lip_sync.enabled`，从 PCM 计算 30Hz envelope，一次 IPC 发给 DesktopPet 驱动 `ParamMouthOpenY`。
- 独立 TTS worker + 最新优先队列（可打断），不阻塞 UI / DesktopPet。
- 「停止」/ 新消息 / 清空对话 / 退出都会停止当前语音。
- TTS 失败不影响 AI 文本、气泡与 history。
- 未来可替换为 SAPI / Edge TTS / Piper / GPT-SoVITS（仅需新增 `impl TtsProvider`）。

## 边界

- 不接触 HWND / Cubism / D3D；不修改桌宠配置；不绕过 IPC。
- LLM 输出经 ExpressionMapper 后才成为协议请求（不允许模型直接控制动作/命令）。
- Provider 失败 / 桌宠离线都不 panic，写入 `pet-agent.log`。

## IPC

- 管道：`\\.\pipe\DesktopPetExpression_v1`
- 协议定义在共享 crate `pet-protocol`（版本化）。
