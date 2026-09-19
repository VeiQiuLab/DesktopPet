# Piper voices

把本地 Piper voice 模型放在此目录（或任意路径，并在配置中指定绝对路径）。

## 本机已下载（仅开发测试，**不提交 Git**）

- `zh_CN-huayan-medium.onnx`
- `zh_CN-huayan-medium.onnx.json`

### 许可证 / 授权

- **Piper Runtime**（`piper1-gpl`，即 `piper-tts` 包）：GNU GPL v3（见上游 `COPYING`）。
  - 说明：GPL 具有传染性。**若未来要随产品分发**，需评估与自有代码的兼容性；
    当前仅用于本机开发测试，未随项目分发。
- **Voice `zh_CN-huayan-medium`**：MODEL_CARD 标注数据集来源
  `PlayVoice/HuaYan_TTS`，**License: Unknown**。
  - 说明：授权状态未明确，**仅用于本机开发测试**，不得假定可再分发。

> 结论：当前阶段 Runtime 与 Voice Model 均只用于本机开发验证，不纳入 Git、不随项目分发。

## 配置示例（pet-agent.config.json）

```json
"tts": {
  "enabled": true,
  "provider": "piper",
  "timeout_secs": 30,
  "piper": {
    "exe": "C:\\AI\\QwenGame\\DesktopPet\\pet-agent\\tools\\piper-env\\Scripts\\python.exe",
    "model": "C:\\AI\\QwenGame\\DesktopPet\\pet-agent\\voices\\zh_CN-huayan-medium.onnx",
    "config": "C:\\AI\\QwenGame\\DesktopPet\\pet-agent\\voices\\zh_CN-huayan-medium.onnx.json"
  }
}
```

## 说明

- pet-agent 只调用**已存在**的 Piper 运行时；不自动下载、不修改 PATH、不改系统环境。
- 用 `pet-agent piper-check` 检查配置；用 `pet-agent tts-test "文本"` 测试声音链路。
- 中文效果需要中文 voice；用英文 voice 只能做技术链路验证。
- **不要**把 `.onnx` 模型或 `tools/` 运行时提交到 Git（已在 .gitignore 排除）。
