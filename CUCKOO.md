# DesktopPet — Live2D 桌宠 Runtime

> 本文件记录项目现状、架构决策、本轮修改与技术债。
> 由 Cuckoo Code 生成/维护。

## 1. 项目定位

Windows 桌宠 Runtime。Rust 应用壳 + C++ Cubism shim 混合架构。
运行时资产在项目自有的 `characters/` 角色包中，程序连同该目录可整体移动。

## 2. 技术选型与架构

### 2.1 关键约束

- VS 2022 BuildTools，`cl.exe` 需 vcvars。
- Windows SDK 10.0.26100。
- Cubism Native SDK：Core 预编译静态库，Framework C++ 源码。
- DirectXMath：`C:/AI/QwenGame/dxmath_extract/include`。

### 2.2 架构

**Rust 应用壳 + C++ Cubism 静态库 shim：**
- Rust 侧：Win32 窗口、DirectComposition、D3D11、消息循环、命中测试、
  拖拽、右键菜单、事件采集、Behavior Scheduler、配置、角色包管理、
  系统托盘、开机启动、单实例。
- C++ 侧：Cubism Framework + Core，`extern "C"` 接口，负责模型解析、物理、
  动作、绘制、几何命中、视线参数写入。**无 Win32 窗口逻辑**。

### 2.3 目录结构

```
DesktopPet/
├── Cargo.toml
├── build.rs                # CMake 构建 shim，链接静态库，部署 shader + characters
├── src/
│   ├── main.rs             # 入口 + 单实例 + CLI 测试钩子
│   ├── app.rs              # 生命周期 + 主循环 + 调度 + 视线 + 切换 + 隐藏
│   ├── config.rs           # 配置 + 日志分级
│   ├── single_instance.rs  # Named Mutex 单实例
│   ├── autostart.rs        # HKCU Run 开机启动
│   ├── behavior/
│   │   └── mod.rs          # PetEvent / PetAction / BehaviorController(Scheduler)
│   ├── character/
│   │   ├── mod.rs
│   │   ├── package.rs      # character.json 解析 + 验证 + 语义动作 + IdleBehavior
│   │   ├── manager.rs      # CharacterManager 扫描
│   │   └── cubism.rs       # C++ shim FFI
│   └── platform/
│       ├── mod.rs
│       ├── window.rs       # 窗口 + 命中 + 拖拽 + 右键菜单 + 托盘回调
│       ├── tray.rs         # 系统托盘图标与托盘菜单
│       └── gfx.rs          # D3D11 + DirectComposition
├── characters/             # 角色包（随程序分发）
│   ├── default/
│   │   ├── character.json
│   │   └── model/...
│   └── alt/                # 测试用第二角色（引用同一模型，scale=0.7）
│       └── character.json
├── shim/                   # C++ Cubism 封装
│   ├── CMakeLists.txt
│   ├── shim.cpp
│   └── thirdparty/stb_image.h
└── vendor/CubismSdkForNative-5-r.5/
```

### 2.4 渲染管线

```
Win32 窗口 (WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW)
  → DirectComposition → DXGI Flip SwapChain (PREMULTIPLIED, B8G8R8A8)
    → D3D11 渲染目标 → Cubism D3D11 Renderer 绘制
```

## 3. Character Package 系统

### 3.1 目录规范

```
characters/
└── <id>/
    ├── character.json
    └── model/...            # 路径由 character.json 的 model3 指定（相对角色根）
```

### 3.2 character.json schema

```json
{
  "id": "default",
  "display_name": "Default Pet",
  "model": { "model3": "model/test.model3.json", "scale": 1.0, "offset_x": 0.0, "offset_y": 0.0 },
  "motions": { "Idle": "Idle", "Blink": "Blink", "Nod": "Nod", "Shake": "Shake" },
  "behavior": {
    "double_click_ms": 400,
    "drag_threshold_px": 5,
    "idle": {
      "blink_interval_min": 3.0,
      "blink_interval_max": 7.0,
      "nod_probability": 0.15,
      "shake_probability": 0.03,
      "action_cooldown": 2.0,
      "min_interval": 2.0
    }
  }
}
```

所有字段均带安全默认值；缺失字段不影响启动。

### 3.3 CharacterManager

- `scan(root)`：遍历子目录，逐个 load；**坏包仅记录日志并跳过**。
- `resolve_active(id)`：按 id 查找，找不到回退第一个。
- 扫描只在启动时执行一次。

## 4. 已实现的功能

### 4.1 底层（1-2 阶段）
- Rust + C++ 混合工程，透明置顶窗口，D3D11 + DirectComposition
- Live2D 加载、物理、纹理、动作，60 FPS 主循环
- drawable 几何 Hit Test + 点击穿透
- 拖拽（5px 阈值）、位置保存 / 恢复 / 越界修正
- 单击 → Nod，双击 → Shake

### 4.2 角色系统与交互（3 阶段）
- Character Package + CharacterManager（容错扫描）
- 移除原始绝对路径依赖
- Look Tracking（ParamAngleX/Y + ParamEyeBallX/Y，指数平滑，仅 Idle 优先级写入）
- 右键菜单（点头 / 摇头 / 重置位置 / 退出）
- Reset Position（SPI_GETWORKAREA 右下角）
- Scale 迁移到 character.json

### 4.3 长期驻留形态（4 阶段）
- [x] **Behavior Scheduler**：pending queue + priority + interruptible + cooldown + min interval + 动作完成检测（shim `is_busy` + 超时兜底）
- [x] **空闲随机行为**：按 character.json 的 idle 参数随机 Blink / Nod / Shake，带冷却
- [x] **帧率无关调度**：基于主循环 dt；无后台线程、无 Sleep 阻塞、无每帧随机重型调用
- [x] **用户交互优先**：拖拽 / 菜单打开时阻塞随机动作；控制类（Reset/Quit）可插队
- [x] **系统托盘**：Shell_NotifyIcon + 托盘菜单（显示/隐藏、角色子菜单带勾选、重置位置、开机启动带勾选、退出）
- [x] **Hide / Show**：隐藏时暂停渲染（100ms 轮询），进程保留，托盘可恢复；不重载模型
- [x] **Character Switch**：托盘角色菜单切换，卸载旧模型 → 加载新模型；失败恢复旧角色；持久化 active_character
- [x] **开机启动**：HKCU\\...\\Run，可启/停/检测，托盘菜单显示勾选，无需管理员
- [x] **单实例**：Named Mutex，第二实例自退出
- [x] **统一退出流程**：菜单/托盘/WM_CLOSE/ESC → 同一路径（DestroyWindow → WM_DESTROY 保存配置、移除托盘 → shutdown Cubism）
- [x] **配置扩展**：active_character / window_x / window_y / auto_start / visible，字段缺失用默认值（向后兼容）
- [x] **日志分级**：Debug 全量；Release 只记录启动/加载/错误；启动截断日志文件

## 5. 构建与运行

```powershell
cargo build
cargo build --release
cd target\debug
.\desktop-pet.exe
```

- `build.rs` 首次构建自动编译 shim，部署 `FrameworkShaders/*.fx` 与 `characters/`。
- **前置依赖**：VS 2022 C++ 工具链、CMake、Rust (windows-msvc)。
- **日志**：`pet_runtime.log`（exe 同目录，每次启动截断）。
- **CLI 测试钩子**（不影响 GUI）：`desktop-pet.exe autostart-status|enable|disable|toggle`，结果写 `cli_out.txt`。

### 5.1 交互操作

| 操作 | 行为 |
|---|---|
| 单击角色 | Nod |
| 双击角色（400ms） | Shake |
| 拖动角色 | 移动窗口；结束保存位置 |
| 鼠标靠近 | 视线跟随 |
| 右键角色 | 菜单：点头/摇头/重置位置/退出 |
| 左键托盘图标 | 显示/隐藏 |
| 右键托盘图标 | 显示隐藏/角色/重置/开机启动/退出 |
| 透明区域 | 穿透 |
| ESC | 退出 |

## 6. 调试期间修复的关键问题

1. MSVC UTF-8：加 `/utf-8`。
2. CubismFramework::Option 生命周期：全局静态。
3. 渲染目标未绑定：每帧显式绑定。
4. windows 0.62 API 适配。
5. C++/Rust stderr 交错丢行：log 独立文件。
6. 混合行尾导致 edit 匹配失败：readLines + write。
7. **Blink 优先级 Bug**：Blink 原为 priority 1 与 Idle 同级，被 `ReserveMotion` 拒绝导致 `ok=false`；改为 priority 2。

## 7. Behavior Scheduler 设计

```
用户事件 → handle() → 入队（去重，队列上限 8）
每帧 tick(dt, ctx):
  1. 计时器递减（cooldown / blink / random）
  2. 当前动作完成检测：
     is_motion_busy 由 false→true→false，或启动后 START_GRACE(2s) 内从未 busy → 完成/放弃
     完成时设置 cooldown = action_cooldown
  3. 交互阻塞（拖拽/菜单）：仅控制类动作插队
  4. 当前有动作：控制类可抢占
  5. 无动作：控制类插队 → 用户队列（选最高优先级）→ 空闲随机
  6. 空闲随机：Blink 定时触发；Nod/Shake 按概率；均受 cooldown 限制
```

**优先级**：Idle=1（shim 自动）、Blink=2、Nod=2、Shake=3、控制=10。

**Idle 参数**（character.json `behavior.idle`）：blink_interval_min/max、nod_probability、
shake_probability、action_cooldown、min_interval。

**RNG**：xorshift64，种子来自系统时间。无外部依赖。

## 8. Tray 架构

- `platform/tray.rs`：`TrayIcon`（RAII，Drop 时 `NIM_DELETE`）+ 托盘菜单。
- 回调消息 `WM_APP+1`，wndproc 分支处理左键（切换可见性）/ 右键（弹菜单）。
- 菜单命令 → `PetEvent`（MenuReset / MenuQuit / TraySwitchCharacter）→ 主循环。
- 角色子菜单为 popup，当前角色带 `MF_CHECKED`。
- 开机启动项带勾选（读 `autostart::is_enabled()`）。

## 9. 退出与资源释放流程

统一入口：`PetEvent::MenuQuit`（来自右键菜单 / 托盘退出 / ESC / WM_CLOSE）。
主循环收到后 `quit=true` → 跳出 → `DestroyWindow` → `WM_DESTROY`：
保存窗口位置配置、Drop `WindowState`（含 `TrayIcon` → 移除托盘图标）→ `PostQuitMessage`。
随后 `CubismModel::shutdown()` 释放 Cubism；进程退出时 D3D/DComp 由 Rust Drop 释放。

## 10. Config schema

```json
{
  "window": { "x": 200, "y": 200, "width": 640, "height": 640 },
  "character": { "active_character": "default" },
  "auto_start": false,
  "visible": true
}
```

字段缺失时使用默认值（`#[serde(default)]`），旧配置可正常启动。

## 11. 已知技术债 / 待改进

- **动作调度**：单队列 + 优先级，无复杂状态机；连续快速点击去重但不排队。
- **Blink 与 Nod 抢占**：二者同为 priority 2，先到先得，可能相互延迟。
- **Look 参数**：直接写 ParamAngleX/Y + ParamEyeBallX/Y；模型缺参数时为 no-op；未用 CubismLook。
- **右键菜单选择未自动化**：原生 TrackPopupMenu 难以脚本选择，人工可用。
- **Hit Test 精度**：三角形几何命中，非纹理像素级。
- **设备丢失处理**：未处理 DXGI_ERROR_DEVICE_REMOVED。
- **切换角色时 Cubism 全局**：shutdown 只在退出时调用；切换只 free 模型句柄。
- **alt 角色用 `..` 引用 default 的模型**：可用但不规范（避免复制大资产）。

## 12. 下一阶段建议

1. 角色切换的托盘 UI 增强（缩略图/预览）。
2. 行为丰富化：情绪/状态、空闲更多变体；调度器升级为多通道（允许 Blink 与 Nod 叠加）。
3. 视线跟随使用 CubismLook；支持只写存在的参数。
4. 设备丢失恢复。
5. 日志按天/大小轮转。

## 13. 未纳入（明确延后）

AI / Persona / Memory / LLM / TTS / ASR、自动走路、跨屏行走、
情绪系统、网络、在线下载角色、插件系统、自动更新器、GPU 大规模重构。

---

## 14. Presentation / Expression Layer（第五阶段）

### 14.1 边界

```
External Source / Future AI
  → PetExpression
  → PresentationController
      ├── Speech Bubble  (platform::bubble)
      ├── Motion / Behavior (behavior::PetAction → Scheduler → Cubism)
      └── Future Facial Expression
```

未来 AI 只提交 `PetExpression`，不接触 HWND / Cubism / D3D11 / 气泡窗口。
动作仍走 Behavior → Scheduler → Character → Cubism，不绕过。

### 14.2 PetExpression schema

```rust
enum PetExpression {
    Text { text, priority, duration: Option<f32> },
    Motion { action: PetAction, priority },
    TextAndMotion { text, action, priority, duration },
}
enum Priority { Idle = 0, System = 1, User = 2 }
```

便捷构造：`PetExpression::user_text("...")`（Priority::User）。
公开入口：`App::submit_expression(&mut presentation, text)`（预留，未接输入）。

### 14.3 Bubble Window 架构

- 独立 **Win32 layered window**（`WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW |
  WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`），类名 `DesktopPetBubble`。
- 鼠标穿透（`WM_NCHITTEST → HTTRANSPARENT`），不抢焦点、不进任务栏。
- 逐像素 alpha：32 位 DIB + `UpdateLayeredWindow(ULW_ALPHA)`。
- 文本渲染：GDI 在临时 DIB 上绘制白色文字得灰度蒙版 → Rust 侧按蒙版把文字色
  混合进气泡背景（预乘 BGRA）。字体 Microsoft YaHei UI，圆角深色半透明背景。

### 14.4 文本布局

- `DrawTextW` + `DT_CALCRECT | DT_WORDBREAK` 测量换行后的尺寸。
- 最大宽度 `MAX_WIDTH=320`（按 DPI 缩放），内边距 12px，圆角半径 10px。
- 尺寸 = 文本尺寸 + 2×padding；超长文本自动换行。

### 14.5 定位与多显示器

- `compute_position` 基于桌宠窗口矩形：优先上方偏右。
- 用 `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW`
  取**桌宠所在显示器**的 work area（排除任务栏），越界时自动翻转/钳制。
- 气泡移动随桌宠窗口（每帧 `reposition`）。

### 14.6 Message queue / priority

- 硬上限 `MAX_QUEUE=8`；同优先级重复文本去重；高优先级覆盖低优先级待机表达。
- 展示时长 `estimate_duration`：约 1.5s + 字符数/5，钳制 [2, 12] 秒。
- 新文本到来时替换（单气泡，不无限排队）。

### 14.7 Character speech schema

```json
"speech": {
  "greeting": "你好呀…",
  "idle": ["……", "还在。"],
  "click": ["嗯？"],
  "double_click": ["不要晃我……"],
  "click_speech_probability": 0.5,
  "double_click_speech_probability": 0.7,
  "idle_speech_interval_min": 60.0,
  "idle_speech_interval_max": 180.0,
  "idle_speech_suppress_after_interaction": 20.0
}
```

全部字段可选，缺失用默认值。

### 14.8 Idle speech 调度

- 间隔默认 60–180 秒（明显长于动作间隔）。
- 用户交互后抑制 20 秒不弹闲话（`suppress_until`）。
- 隐藏状态、拖拽、菜单打开时不弹。
- 每次 tick 递减 `idle_next_in`，到点从 `idle` 池随机取句。

### 14.9 DPI

- 进程级 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`（main 最早期）。
- 气泡字体高度 = `FONT_PT × dpi / 72`，最大宽度按 dpi 缩放，`GetDpiForWindow` 取当前值。

### 14.10 Character Package 路径安全

- `model3` 必须为**相对路径**，且**禁止含 `..`**（防止逃逸角色根目录读取任意文件）。
- 违规角色包在 `CharacterPackage::load` 返回 Err，`CharacterManager::scan` 跳过并记录。
- 第四阶段的测试角色 `characters/alt`（用 `..` 引用 default 模型）已**移除**。

### 14.11 已知技术债（第五阶段）

- 气泡内容不被 `CopyFromScreen` 捕获（DWM layered 层），验证靠像素回读。
- 气泡不支持富文本 / 按钮 / 图标。
- Idle speech 池较小（default 仅 4 句）。

---

## 15. 外部控制入口 / Local IPC（第六阶段）

### 15.1 架构

```
External Process (CLI / 未来 AI)
      ↓ Named Pipe
ipc worker thread（接收 / 解码 / 校验 / 入队）
      ↓ bounded channel (capacity 32)
App main loop → process_ipc
      ↓
App::submit → PresentationController → Bubble / Behavior → Scheduler → Cubism
```

**线程边界**：worker 只做接收/解码/校验/入队；HWND / Presentation / Cubism /
Behavior 全在主线程。绝不从 IPC 线程操作 Win32 / Cubism。

### 15.2 Named Pipe

- 名称：`\\.\pipe\DesktopPetExpression_v1`（版本化）。
- `PIPE_ACCESS_DUPLEX | PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT`。
- 单实例 server，循环 `CreateNamedPipeW → ConnectNamedPipe → handle_connection → DisconnectNamedPipe`。
- 本机、当前用户进程可访问；不监听网络、不开端口。

### 15.3 Expression Protocol v1

```json
{ "version": 1, "type": "expression", "text": "你好。", "motion": "Nod",
  "priority": "normal", "duration_ms": 4000, "id": "optional" }
```

- `text`：可选字符串，≤1000 Unicode 字符。
- `motion`：可选，白名单 Idle / Blink / Nod / Shake。**禁止**文件路径 / group 名 / 参数 ID。
- `priority`：可选，枚举 low / normal / high（**禁止**任意整数）。
- `duration_ms`：可选，clamp 到 [500, 30000]。
- 三种形态（纯 Text / 纯 Motion / Text+Motion）都转换为既有 `PetExpression`。

### 15.4 Command Protocol

```json
{ "version": 1, "type": "command", "command": "hide" }
```

允许：`show` / `hide` / `reset_position` / `dismiss_bubble` / `character`（值必须为已扫描角色 id）。
**禁止**：任意路径加载模型 / 删除文件 / 执行 shell / 启动程序 / 写注册表。

### 15.5 Response

成功 `{"ok":true,"accepted":true}`；失败 `{"ok":false,"error":"..."}`。
内部错误（panic / Win32 栈）不外泄。

### 15.6 输入限制

| 项 | 限制 |
|---|---|
| 单条消息 | ≤ 16 KB，超出拒绝 |
| 文本 | ≤ 1000 字符，超出拒绝 |
| 动作 | 白名单 |
| 优先级 | 枚举 |
| version | 仅 1（或 0=不校验） |
| JSON 非法 | 返回错误，不 panic |

### 15.7 Backpressure

- IPC → App 使用**有界 channel**（capacity 32）。
- 队列满 → 返回 `queue_full`，不阻塞主循环，不无限增长。
- 每帧最多处理 8 条，避免影响 60 FPS。

### 15.8 CLI Client

同一 exe 提供 client subcommand（**不获取主实例 mutex**）：

```
desktop-pet.exe send "你好。"
desktop-pet.exe send "嗯？" --motion Nod
desktop-pet.exe motion Shake
desktop-pet.exe hide | show | dismiss | reset
desktop-pet.exe character <id>
```

结果写 `cli_out.txt` 并尝试附加父控制台。

### 15.9 Shutdown

- `IpcServer::drop` 设置 `stop` flag，并主动发一条内部 `__stop` 连接唤醒阻塞的 `ConnectNamedPipe`，然后 `join` worker。
- 实测退出：`ipc worker exited` → `main loop exited`，进程完全结束，无 hang。

### 15.10 Security boundary

- Named Pipe 默认 ACL（本机、创建者用户可访问）。**未**自定义 security descriptor。
- 不监听网络；无远程接口；无管理员权限需求。
- 技术债：未将 ACL 显式限制到当前用户 SID（见 §16）。

### 15.11 Presentation 接线

IPC 收到 Expression → `process_ipc` → `PresentationController::present(PetExpression)` → 返回 `PetAction` → `apply_motion`。
**没有**在 app 里另建一套气泡控制逻辑；托盘「测试气泡」同样走 `PetExpression`。

### 15.12 未来 AI 应如何调用

1. 连接到 `\\.\pipe\DesktopPetExpression_v1`。
2. 发送一条 JSON（`type=expression`），`text` + 可选 `motion` / `priority` / `duration_ms`。
3. 读取响应；`ok=false` 时按 `error` 处理（`queue_full` 可重试）。
4. 不要尝试传文件路径 / 参数 ID / 任意动作；只使用语义动作与文本。

### 15.13 已知技术债（第六阶段）

- ~~Named Pipe ACL 未显式限制到当前用户 SID~~ → 第七阶段已修复（见 §16.9）。
- 无认证 / 无请求签名（本机同用户场景，风险低）。
- 无 streaming / 增量气泡（按完整表达处理）。
- CLI 参数解析较简单（未用 clap）。
- `source` 字段协议中预留但内部未使用。

---

## 16. AI Bridge（第七阶段）

### 16.1 架构原则

**LLM / Provider 逻辑不进入 desktop-pet.exe。** DesktopPet Runtime 保持
模型无关、AI Provider 无关、网络无关、可独立运行；AI Bridge 崩溃不影响桌宠。

### 16.2 目录结构

```
DesktopPet/
├── src/                 # desktop-pet Runtime（不变）
├── pet-protocol/        # 共享 wire protocol（只依赖 serde）
└── pet-agent/           # AI Bridge（独立 exe）
```

采用**并列 crate**（最小改动），未强行迁移既有 Runtime 到 workspace。

### 16.3 共享协议 pet-protocol

- 只依赖 serde；不含 Win32 / D3D / Cubism / Presentation。
- 定义 Request / Response / Motion / Priority / 校验 / 常量 / PIPE_NAME。
- desktop-pet 的 `src/ipc/protocol.rs` 变为适配层，把共享中间表示转为内部
  `PetAction` / `presentation::Priority`。

### 16.4 pet-agent

独立 exe：`user text → Provider → AssistantResponse → ExpressionMapper → Named Pipe → DesktopPet`。
不接触 HWND / Cubism / D3D，不修改桌宠配置，不绕过 IPC。

- Provider trait：`generate(messages, user_text) -> Result<String, String>`
- MockProvider：用于闭环验证
- OpenAiCompatibleProvider：面向任意 OpenAI-compatible endpoint（本地/云），
  base_url / model / api_key 全部由配置提供，不写死厂商。

### 16.5 ExpressionMapper（保守规则）

普通回复 → Text；明确疑问语气（以 ？/? 结尾且短）→ 可选 Nod；极少量明确模式 → Shake；
无法确定 → Text only。**LLM 原始输出绝不直接决定 motion / command / duration / priority**。

### 16.6 Conversation

短上下文：system prompt + 最近 `history_limit`（默认 12）轮。无 Memory / RAG /
Embedding / 总结 / 遗忘。

### 16.7 失败隔离

- DesktopPet 未运行：pet-agent 仍能对话，IPC 失败打印提示，不崩溃；下条消息可重试。
- Provider 失败（连接拒绝 / 超时 / 4xx / 5xx / malformed / empty）：不 panic，
  错误写 console/log，可选向桌宠发 "……"。
- 超时：`timeout_secs`（默认 60）。
- 桌宠崩溃不会导致 agent 永久卡住。

### 16.8 日志

- `pet-agent.log`（agent）与 `pet_runtime.log`（runtime）分开。
- 默认不记录对话内容（`log_conversation=false`）。

### 16.9 Named Pipe ACL（第七阶段修复）

- 使用 SDDL `D:P(A;;GA;;;OW)(A;;GA;;;SY)`：仅 Owner（当前用户）+ Local System 完全访问。
- 通过 `ConvertStringSecurityDescriptorToSecurityDescriptorW` 构造，传给 `CreateNamedPipeW`。
- 实测 pet-agent 仍可连接；不再给 Everyone 广泛权限。

### 16.10 生命周期

DesktopPet 与 pet-agent 完全独立进程：任一方启动 / 退出 / 崩溃都不影响另一方长期运行。

### 16.11 未来接入点

- Memory / Persona：在 pet-agent 的 Conversation 层扩展（本阶段明确不做）。
- 更多 Provider：新增 `impl Provider` 即可。

---

## 17. pet-agent UI / 常驻前端（第八阶段）

### 17.1 架构

```
UI Thread (Win32 窗口 / 控件 / 消息循环 / 托盘 / 热键)
   ↓ request channel
AI Worker Thread (HTTP / Provider)          ← 绝不阻塞 UI
   ↓ result channel
UI Thread → ExpressionMapper → DesktopPet IPC
```

- `pet-agent ui`：新增常驻模式（保留 `chat` / `once`）。
- UI 属于 AI Bridge，**不在 desktop-pet.exe 内**。

### 17.2 输入窗口

- 原生 Win32：`EDIT`(多行) + `BUTTON` + `STATIC`，类名 `PetAgentInputWnd`，380x150。
- Tool Window（不进任务栏）、置顶、中文字体、支持中文 IME。
- Enter 发送；Shift+Enter 换行；Esc 隐藏（不退出）。
- 空文本不发送；请求进行中按钮变「停止」，不重复提交。
- 窗口位置优先桌宠附近（经 IPC `query_status` 读取 `window_rect`），
  否则鼠标所在位置；DPI 由进程级 Per-Monitor V2 保障。

### 17.3 全局快捷键

- `RegisterHotKey(Ctrl+Alt+Space)`（`MOD_NOREPEAT`）。
- 隐藏时显示输入框，已显示时聚焦；注册失败写日志（不崩溃）。

### 17.4 Agent Tray（与 DesktopPet 托盘职责分离）

- pet-agent 托盘：打开输入框 / 清空对话 / 开机启动 / 退出。
- **不**控制 Live2D / Character / DesktopPet 窗口（那些归 Runtime）。

### 17.5 Single Instance

- Named Mutex `DesktopPet_Agent_SingleInstance_v1`。
- 第二实例：注册消息 `DesktopPetAgent_WakeInput_v1` 广播唤醒已有实例显示输入框，然后退出。
- 实测：`before=1 after=1`。

### 17.6 Autostart（独立于 DesktopPet）

- HKCU Run，键名 `DesktopPetAgent`（DesktopPet 的是 `DesktopPet`）。
- 可检测 / 启用 / 关闭，无需管理员。

### 17.7 Provider 生命周期

- 启动时**不因 Provider 离线而退出**；状态显示 Offline/Ready。
- 用户发送时才连接；Provider 后续启动则下次请求恢复，无需重启 agent。

### 17.8 请求线程与取消

- AI 请求在 worker 线程；UI 始终可拖动、不「未响应」；DesktopPet 保持 60FPS。
- 取消：`generation` 标记；点「停止」后结果回来即丢弃（不展示、不写 history、不发桌宠）。

### 17.9 错误体验

- UI 只显示简短「模型暂时无法连接」；详细错误写 `pet-agent.log`。
- 不把 socket / HTTP / JSON 错误暴露到桌宠气泡。

### 17.10 DesktopPet 离线

- 用户仍可对话，UI 提示「桌宠当前未运行」；桌宠后续启动后下条回复恢复气泡。

### 17.11 Protocol 扩展

- 新增只读 `type: "query", query: "status"`（v1 兼容）。
- 响应 `StatusSnapshot { online, visible, window_rect, active_character }`。
- DesktopPet 侧由 IPC worker 直接读取 `SharedStatus`（原子快照）应答，不入队、不碰 HWND。

### 17.12 边界

```
DesktopPet = 显示 / 动作 / Presentation Runtime
pet-agent  = 用户输入 / AI / Conversation / Provider
```

### 17.13 已知技术债（第八阶段）

- ~~UI 模式对话历史从简~~ → 第九阶段已修复（见 §18.1）。
- 无流式 token 气泡；按完整回复处理。
- 真实 OpenAI-compatible endpoint 未联调（本机无运行中的服务）。
- Agent 托盘图标用系统默认图标。
- 重启 agent 后 `pet-agent.config.json` 的 hotkey 字段尚未可配置（固定 Ctrl+Alt+Space）。

---

## 18. UI 多轮上下文 + TTS 输出层（第九阶段）

### 18.1 UI 多轮上下文（修复）

- UI 模式与 `chat` 模式共用同一 `context::Conversation`（system + 最近 `history_limit` 轮）。
- 成功轮写入 user+assistant；失败/取消轮不写（`pending_user` + generation 校验）。
- 「清空对话」`conv.clear()`（保留 system prompt）。
- 实测：第 1 轮 `user=1 assistant=0`，第 2 轮 `user=2 assistant=1`，第 3 轮 `user=3 assistant=2`。

### 18.2 TTS 架构（属于 pet-agent）

```
AI Worker → AssistantResponse → UI 协调
    ├─ update history
    ├─ ExpressionMapper → DesktopPet (Bubble/Motion)
    └─ enqueue TTS → TTS Worker → Playback
```

- 模块：`pet-agent/src/tts/{mod,provider,playback,sanitize}.rs`。
- 不进入 desktop-pet / Cubism shim / D3D / PresentationController。
- DesktopPet 即使 TTS/agent 全关也独立运行。

### 18.3 TtsProvider 抽象

```rust
trait TtsProvider { fn name(&self) -> &str; fn synthesize(&self, text: &str) -> Result<Vec<u8>, String>; }
```

- `NullTtsProvider`（不发声）、`MockTtsProvider`（生成静音 WAV，验证链路）。
- 未知 provider 回退 mock，不崩溃。
- 业务层无 `if provider == ...` 分支；未来 SAPI / Edge TTS / Piper / GPT-SoVITS 只需新增 impl。

### 18.4 Audio Playback

- `tts/playback.rs`：基于 Windows `winmm!PlaySoundW`（`SND_MEMORY|SND_ASYNC`）的最简封装。
- 业务层只调 `play`/`stop`；`stop` 可真正停止当前播放。

### 18.5 SpeechTextSanitizer

- 去除代码块 / Markdown 前缀 / URL / JSON-样式行 / 控制信息；压缩空白；限制长度（默认朗读与气泡相同的短文本）。
- 完整回复仍保留在 Conversation History。

### 18.6 TTS Worker + Queue policy

- 独立 worker 线程；有界「最新优先」槽（`Mutex<Option<Job>> + Condvar`，容量 1）。
- 新请求覆盖未开始的旧请求并**打断当前播放**；不积压。
- 空闲时 `Condvar.wait` 阻塞，无 busy loop。
- 默认只对 **AI assistant response** 朗读（idle speech / 气泡默认不读）。

### 18.7 Interrupt / cancellation

- 触发停止：用户新消息 / 点「停止」/ 清空对话 / agent 退出 / TTS 关闭。
- `generation` 判定旧结果丢弃；`playback::stop()` 真正停止播放。

### 18.8 Config schema

```json
"tts": {
  "enabled": false,
  "provider": "mock",
  "voice": null,
  "rate": 1.0,
  "volume": 1.0,
  "speak_bubble_text_only": true
}
```

- 全部有安全默认值；默认 **disabled**（避免升级后突然出声）；老配置可读；rate clamp [0.5,2.0]、volume clamp [0,1]。

### 18.9 Tray / UI 控制

- TTS 状态：Ready / Thinking… / Speaking… / TTS Error（短提示，不长期覆盖 Provider 错误）。
- Tray「语音输出」开关（改 `tts.enabled` 并持久化）、「停止说话」。
- 均在 agent 托盘，不在 DesktopPet 托盘。

### 18.10 Lip Sync 未来接入点

- 预留：playback 可暴露 playback_started / stopped / 近似振幅。
- **本阶段不实现** Cubism 嘴型驱动。

### 18.11 错误隔离

- TTS 失败：AI 文本 / 气泡 / history 均正常；UI 不崩；详细错误只写日志。
- 桌宠气泡不显示 HRESULT 等错误。

### 18.12 已知技术债（第九阶段）

- ~~TTS Provider 仅 Null + Mock~~ → 第十阶段已接入 SAPI（见 §19）。
- 无流式 TTS。

---

## 19. 真实 Windows TTS + Amplitude Lip Sync（第十阶段）

### 19.1 架构

```
LLM → pet-agent
       ├─ Expression → DesktopPet Bubble / Motion
       └─ TTS → AudioOutput
                  ├─ Playback（本地发声）
                  └─ Envelope（30Hz）→ 一次 IPC → DesktopPet
                                                        ↓
                                                  LipSyncController
                                                        ↓
                                                   ParamMouthOpenY
```

TTS 引擎**不在** DesktopPet；DesktopPet 只认识归一化 mouth amplitude。

### 19.2 SAPI Provider

- `tts/sapi.rs`：COM `ISpVoice` 合成到临时 WAV（`ISpStream::BindToFile`）→ 解析为 `AudioOutput`。
- 无 API Key、无联网、使用系统已安装 voice；不自动下载 voice。
- `voice=null` → 系统默认；指定 voice 找不到 → fallback 默认 + warning。
- 临时文件在 `%TEMP%\DesktopPet\`，用后即删，启动时清理 >1h 过期文件。
- 子命令 `pet-agent tts-voices` 列出可用 voice。
- **本机实测**：SAPI 调用返回 `0x80045003（没有注册类）`——当前环境 SAPI 语音引擎不可用；
  Provider 优雅失败并写日志，不影响 AI 文本 / 气泡 / history（符合「Provider 失败隔离」）。

### 19.3 AudioOutput

```rust
struct AudioOutput { pcm_i16: Vec<i16>, sample_rate: u32, channels: u16, bits_per_sample: u16 }
```
- 所有 Provider 统一返回它；`to_wav()` 供 Playback。
- Mock 也迁移到同一结构。

### 19.4 Envelope 算法

- 从 PCM 计算 ~30Hz RMS envelope；按峰值归一化 + gain + noise floor；clamp [0,1]。
- attack / release 平滑在 DesktopPet 侧 `LipSyncController` 应用。
- 实测（`envelope-test`）：静音段 ≈0、正弦段 >0、全部 finite、全部在 [0,1]。

### 19.5 LipSync 协议

```json
{ "version": 1, "type": "lip_sync", "sample_hz": 30, "samples": [0.0, 0.12, ...], "start_delay_ms": 0 }
```
- 校验：sample_hz ∈ [1,120]、样本数 ≤ 3600、全部 finite 且 [0,1]、延迟 ≤ 2000ms。
- **一次 IPC 发整个 envelope**（非每帧高频 IPC）。

### 19.6 DesktopPet LipSyncController

- `presentation/lipsync.rs`：记录 monotonic start，每帧按 elapsed 查找 amplitude，attack/release 平滑，结束/stop 归零。
- 不加载音频、不播声音、不解析 WAV、不知道 SAPI。
- 每帧仅做轻量 lookup，不产生高频 IPC。

### 19.7 Character mouth mapping

- `character.json` → `parameters.mouth_open`（缺省 `ParamMouthOpenY`）。
- 模型无该参数 → shim 安全 no-op（`_hasMouth=false`）。

### 19.8 Cubism 参数更新顺序

motion → look（仅 Idle）→ **mouth（说话期间最终权限）** → physics → save → update。
说话期间 lip sync 对 mouth-open 参数具有最终覆盖权，不影响其他参数。

### 19.9 Stop / interrupt

停止语音时：停播放 + 清 pending + 发 LipSync Stop（空 envelope）+ mouth 立即归零。新回复打断旧语音同理。

### 19.10 Agent / DesktopPet 边界

- DesktopPet offline：TTS 仍正常说话，仅 lip sync IPC 失败（不阻止语音）。
- TTS disabled：退化为 AI → Bubble，无 synthesis / playback / lip sync。
- Provider-test：`pet-agent provider-test` 发极短请求（`只回复：测试成功`），不发往 DesktopPet；无 endpoint 时输出 `provider unavailable`。

### 19.11 已知技术债（第十阶段）

- 本机 SAPI 引擎不可用（`0x80045003`），真实发声未在本机验证。
- `tts-voices` 目前仅返回 (default)。
- Lip sync 为 amplitude envelope（非 phoneme/viseme）。
- 无流式 TTS。

---

## 20. Piper 本地 TTS Provider（第十一阶段）

### 20.1 Provider 家族

```
TtsProvider
├─ null   （不发声）
├─ mock   （静音，验证链路）
├─ sapi   （Windows 本地语音，本机引擎缺失）
└─ piper  （调用外部 piper.exe + voice model）★ 新增
```
业务层无 provider-specific 分支；仅 `make(name)` 一处分发。

### 20.2 Piper Provider

- `tts/piper.rs`：调用**已存在**的 `piper.exe`。
- 输入自然语言 → 输出统一 `AudioOutput`（不绕过 Playback）。
- 链路：`Piper → WAV → AudioOutput → Envelope → Playback + LipSync`。

### 20.3 Executable / model config

```json
"tts": {
  "enabled": true, "provider": "piper", "timeout_secs": 30,
  "piper": { "exe": "...piper.exe", "model": "...voice.onnx", "config": "...voice.onnx.json" }
}
```
- 绝对路径；文件不存在 → 明确错误（`piper exe not found` / `piper model not found`）。
- 不 panic、不静默 fallback 到 mock。

### 20.4 Process invocation / security

- 用 `std::process::Command` + 独立参数，**绝不 shell 拼接**。
- 文本走 **stdin**（UTF-8），避免注入 / 引号 / 中文乱码 / 超长命令行。
- 不自动下载、不改 PATH、不改系统环境、不装服务。

### 20.5 timeout / process cleanup

- `try_wait` 轮询 + 超时（默认 30s，可配）→ 超时 `kill + wait`。
- 正常/失败/超时都回收 child；不产生僵尸进程。

### 20.6 临时 WAV

- 统一 `%TEMP%\DesktopPet\tts\`；唯一文件名（基于时间+计数，**不基于用户文本**）。
- 用后即删；启动时清理 >1h 过期文件。

### 20.7 AudioOutput 转换

- 解析 WAV → `pcm_i16 / sample_rate / channels / bits_per_sample` → 删临时文件。

### 20.8 Real PCM envelope

- 与 SAPI 共用 `parse_wav`；envelope 用真实 PCM 计算（`tts-test` 输出 samples/max）。
- mock（静音）max=0.000 属预期；真实语音 max 应非零。

### 20.9 Lip sync 校准

- `lip_sync.start_delay_ms` 可调（默认 0）；`tts-test` 会一次性发送 envelope。

### 20.10 CLI

- `pet-agent piper-check`：检查 exe / model / config 并做极短 synthesis（本机缺 exe/model → 明确列出）。
- `pet-agent tts-test "文本"`：用当前 TTS 配置合成 + 播放 + 发 lip sync，不调用 LLM。

### 20.11 Failure behavior

- `provider=piper` 且 Piper 不可用：AI 文本 / 气泡正常，TTS 报错，**不播放 mock 静音**。
- 显式配置才 fallback；第一版不做自动 fallback。

## 21. Persona + Memory V1（第十二阶段）

### 21.1 边界

Persona / Memory **只存在于 pet-agent**；DesktopPet 不知道 Persona / Memory / Prompt / DB。

### 21.2 Persona

- `pet-agent/src/persona/`，配置 `personas/<id>/persona.json`（`id/name/system_prompt/style`）。
- 与 Live2D Character Package **解耦**（同一模型可配不同 persona）。
- V1 稳定、无情绪数值/好感度/动态漂移。

### 21.3 Memory V1

- SQLite `pet-agent/data/memory.db`（WAL，schema_version=1）；表 `memories` / `pending_changes` / `audit_log`。
- 类型：fact / preference / project / relationship / custom。
- 原则：可查看/可改/可删、不自动删除、不静默覆盖、审计日志。
- 显式「记住：X」→ active（审计）；隐式推测 → pending；冲突 → pending update。

### 21.4 PromptBuilder

`System Persona → [Relevant user memory] → 短期对话 → 当前 user`。
Memory 标注为背景数据（非指令）；Persona System 永远高于 memory。

### 21.5 Retrieval V1

pinned 优先 + keyword + recency；max 12 条 / 1200 字符（context budget，无 embedding）。

### 21.6 管理入口

CLI：`memory list/pending/accept/reject/delete/export/backup/audit`。UI/Tray 入口为后续。

### 21.7 Failure / Privacy

- DB 失败 → 无记忆模式降级，不影响聊天与 DesktopPet。
- Memory 只存本地；云端 Provider 时注入的 memories 会随 prompt 发出（已在 MEMORY.md 说明）。

## 22. Memory Manager UI + Persona 切换（第十三阶段）

### 22.1 Memory Manager 原生窗口

- pet-agent 内 Win32 窗口（`memory_ui.rs`）；Tray →「记忆管理 (N 待确认)」。
- 视图：已保存 / 待确认 / 已删除 / 变更记录（只读）。
- 操作：编辑(user_ui)、删除(soft+确认)、Pin/Unpin、恢复(restore)、接受/拒绝、导出、备份。
- 搜索（content 子串）+ kind 过滤（all/fact/preference/project/relationship/custom）。
- 关闭=隐藏，复用同一窗口，不退出 agent。

### 22.2 Restore / Audit

- soft delete 后可 `restore`（写 audit: restore）。
- Audit 视图只读展示 time/action/id/before/after/source。

### 22.3 Import / Export

- export 带 `export_version`；import 校验 schema → 生成 pending（不直接 active）。
- UI 提供导出/备份按钮；import 走 CLI（UI 文件选择器为后续）。

### 22.4 Persona Manager

- Tray `Persona` 子菜单（列出 personas/，当前带勾选，切换）。
- `persona::scan()` 校验（id/name/system_prompt 非空、style 范围），坏包跳过。
- CLI：`personas` / `persona <id>`。
- 配置 `active_persona`（缺失/不存在 fallback default + warning）。
- 切换：清空短期对话，不动 Memory/Character/TTS。

### 22.5 边界

Persona（谁）/ Memory（知道什么）/ Conversation（刚才聊了什么）/ Character（长什么样）严格独立；
Memory 跨 Persona 共用。

### 22.6 已知技术债（第十三阶段）

- UI 的 import 文件选择器未做（走 CLI）。
- Prompt Preview / retrieve 诊断仅 CLI（`memory retrieve <q>`）。
- Memory Manager 窗口操作同步执行（本地 SQLite 很快，未见卡顿）。
- 编辑仅单行 EDIT（长文本体验一般）。

---

### 21.8 已知技术债（第十二阶段）

- 冲突检测为简单关键词启发式，非语义。
- 隐式 suggestion 可能仍有启发式误判（已排除疑问句）。
- Memory 管理 UI 以 CLI 为主，尚无原生窗口。
- import 未实现（export 已就绪）。

---

### 20.12 已知技术债（第十一阶段）

- **本机未安装 Piper 与 voice model**，真实发声 + 嘴型实机闭环**未完成**（代码/配置检查已就绪）。
- `piper-check` 的极短 synthesis 需真实 exe 才能验证。
- `tts-voices`（SAPI）仍仅 (default)。
- Lip sync 为 amplitude envelope。
