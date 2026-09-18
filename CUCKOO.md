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

AI / Persona / Memory / LLM / TTS / ASR、文字气泡、自动走路、跨屏行走、
情绪系统、网络、在线下载角色、插件系统、自动更新器、GPU 大规模重构。
