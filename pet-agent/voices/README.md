# Piper voices

把本地 Piper voice 模型放在此目录（或任意路径，并在配置中指定绝对路径）。

需要的文件（每个 voice 两个）：

```
voices/
├── zh_CN-xxx.onnx          # 模型
└── zh_CN-xxx.onnx.json     # 配置
```

## 配置示例（pet-agent.config.json）

```json
"tts": {
  "enabled": true,
  "provider": "piper",
  "piper": {
    "exe": "C:\\path\\to\\piper.exe",
    "model": "C:\\AI\\QwenGame\\DesktopPet\\pet-agent\\voices\\zh_CN-xxx.onnx",
    "config": "C:\\AI\\QwenGame\\DesktopPet\\pet-agent\\voices\\zh_CN-xxx.onnx.json"
  }
}
```

## 说明

- **不要**把 `.onnx` 模型提交到 Git（已在 .gitignore 排除）。
- pet-agent 只调用**已存在**的 Piper exe 与模型，不自动下载、不修改 PATH、不改系统环境。
- 用 `pet-agent piper-check` 检查配置；用 `pet-agent tts-test "文本"` 测试声音链路。
- 中文效果需要中文 voice；用英文 voice 只能做技术链路验证。
