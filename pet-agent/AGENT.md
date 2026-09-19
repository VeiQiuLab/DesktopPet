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

## 边界

- 不接触 HWND / Cubism / D3D；不修改桌宠配置；不绕过 IPC。
- LLM 输出经 ExpressionMapper 后才成为协议请求（不允许模型直接控制动作/命令）。
- Provider 失败 / 桌宠离线都不 panic，写入 `pet-agent.log`。

## IPC

- 管道：`\\.\pipe\DesktopPetExpression_v1`
- 协议定义在共享 crate `pet-protocol`（版本化）。
