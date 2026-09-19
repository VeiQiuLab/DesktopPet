# DesktopPet — Live2D 桌宠 Runtime

> Windows 桌面宠物：单进程、无控制台、无子进程。
> Rust 应用壳 + C++ Cubism shim。运行时资产在 `characters/`，程序连同该目录可整体移动。

## 1. 项目定位

一个轻量的 Windows 桌宠。用户双击 `DesktopPet.exe`，角色出现在桌面，常驻直到退出。
纯本地，无网络，无 AI，无子进程。

## 2. 技术架构

### 2.1 前置依赖（构建）

- VS 2022 BuildTools，`cl.exe` 需 vcvars。
- Windows SDK 10.0.26100。
- Cubism Native SDK（Core 预编译静态库 + Framework C++ 源码），置于 `vendor/`。
- Rust（windows-msvc target）。

### 2.2 结构

**Rust 应用壳 + C++ Cubism 静态库 shim：**

- Rust 侧：Win32 窗口、DirectComposition、D3D11、消息循环、命中测试、
  拖拽、右键菜单、事件采集、Behavior Scheduler、配置、角色包管理、
  系统托盘、开机启动、单实例。
- C++ 侧：Cubism Framework + Core，`extern "C"` 接口，负责模型解析、物理、
  动作、绘制、几何命中、视线参数写入。无 Win32 窗口逻辑。

### 2.3 目录结构

```
DesktopPet/
├── Cargo.toml
├── build.rs                # CMake 构建 shim，链接静态库，部署 shader + characters
├── src/
│   ├── main.rs             # 入口 + 单实例 + autostart CLI 钩子
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
│   ├── presentation/
│   │   ├── mod.rs
│   │   ├── controller.rs   # 气泡生命周期 + idle 台词调度
│   │   └── expression.rs   # PetExpression / Priority
│   └── platform/
│       ├── mod.rs
│       ├── window.rs       # 窗口 + 命中 + 拖拽 + 右键菜单 + 托盘回调
│       ├── tray.rs         # 系统托盘图标与托盘菜单
│       ├── bubble.rs       # 文字气泡（layered window）
│       └── gfx.rs          # D3D11 + DirectComposition
├── characters/             # 角色包（随程序分发）
├── shim/                   # C++ Cubism 封装
│   ├── CMakeLists.txt
│   ├── shim.cpp
│   └── thirdparty/stb_image.h
└── vendor/CubismSdkForNative-5-r.5/   # 需自行放入（不入库）
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
  "model": { "model3": "model/test.model3.json", "scale": 0.75, "offset_x": 0.0, "offset_y": 0.0 },
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
  },
  "speech": {
    "greeting": "你好呀，我是你的桌面伙伴。",
    "idle": ["……", "还在。", "嗯？"],
    "click": ["嗯？", "怎么了？"],
    "double_click": ["不要晃我……"],
    "click_speech_probability": 0.5,
    "double_click_speech_probability": 0.7,
    "idle_speech_interval_min": 60.0,
    "idle_speech_interval_max": 180.0,
    "idle_speech_suppress_after_interaction": 20.0
  }
}
```

所有字段均带安全默认值；缺失字段不影响启动。

### 3.3 CharacterManager

- `scan(root)`：遍历子目录，逐个 load；坏包仅记录日志并跳过。
- `resolve_active(id)`：按 id 查找，找不到回退第一个。
- 扫描只在启动时执行一次。

## 4. 已实现功能

- Rust + C++ 混合工程，透明置顶窗口，D3D11 + DirectComposition
- Live2D 加载、物理、纹理、动作，60 FPS 主循环（隐藏时降到 10 FPS）
- drawable 几何 Hit Test + 点击穿透
- 拖拽（5px 阈值）、位置保存 / 恢复 / 越界修正
- 单击 → Nod，双击 → Shake
- Look Tracking（ParamAngleX/Y + ParamEyeBallX/Y，指数平滑，仅 Idle 优先级写入）
- 右键菜单（点头 / 摇头 / 重置位置 / 退出）
- Reset Position（SPI_GETWORKAREA 右下角）
- 行为调度（pending queue + priority + interruptible + cooldown + min interval）
- 空闲随机行为（Blink / Nod / Shake，带冷却）
- 系统托盘（显示/隐藏、角色切换、测试气泡、重置位置、开机启动、退出）
- Hide / Show（隐藏时暂停渲染，进程保留）
- Character Switch（托盘切换，卸载旧模型 → 加载新模型）
- 开机启动（HKCU\\...\\Run）
- 单实例（Named Mutex）
- 统一退出流程（菜单/托盘/WM_CLOSE/ESC）
- 配置扩展（active_character / window_x / window_y / auto_start / visible）
- 日志分级（Debug 全量；Release 只记录启动/加载/错误）

## 5. 构建与运行

```powershell
cargo build
cargo build --release

# Release 产物位置：target/release/desktop-pet.exe
```

- `build.rs` 首次构建自动编译 shim（需要 cmake + VS2022），部署 `FrameworkShaders/*.fx` 与 `characters/` 到输出目录。
- **日志**：`pet_runtime.log`（exe 同目录，每次启动截断）。
- **配置**：`desktop-pet.config.json`（exe 同目录，首次启动生成）。
- **CLI 钩子**（不影响 GUI）：`desktop-pet.exe autostart-status|enable|disable|toggle`，结果写 `cli_out.txt`。

### 5.1 交互操作

| 操作 | 行为 |
|---|---|
| 单击角色 | Nod |
| 双击角色（400ms） | Shake |
| 拖动角色 | 移动窗口；结束保存位置 |
| 鼠标靠近 | 视线跟随 |
| 右键角色 | 菜单：点头/摇头/重置位置/退出 |
| 左键托盘图标 | 显示/隐藏 |
| 右键托盘图标 | 显示隐藏/角色/测试气泡/重置位置/开机启动/退出 |
| 透明区域 | 穿透 |
| ESC | 退出 |

## 6. 调试期间修复的关键问题

1. MSVC UTF-8：加 `/utf-8`。
2. CubismFramework::Option 生命周期：全局静态。
3. 渲染目标未绑定：每帧显式绑定。
4. windows 0.62 API 适配。
5. C++/Rust stderr 交错丢行：log 独立文件。
6. 混合行尾导致 edit 匹配失败：readLines + write。
7. Blink 优先级 Bug：Blink 原为 priority 1 与 Idle 同级，被 `ReserveMotion` 拒绝；改为 priority 2。
8. 头发边缘暗边/白边：纹理 straight alpha → premultiply RGB。
9. 控制台黑框：`#![windows_subsystem = "windows"]` + `CREATE_NO_WINDOW`。

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

## 8. Tray 架构

- 托盘图标在 `platform/tray.rs`，命令 ID 常量导出。
- 菜单项：显示/隐藏、角色子菜单、测试气泡、重置位置、开机启动、退出。
- 回调：`WM_APP + 1`，事件经 channel 送给主线程。

## 9. 退出与资源释放

- 统一入口：菜单退出 / 托盘退出 / WM_CLOSE / ESC → 同一路径。
- `DestroyWindow` → `WM_DESTROY` 保存配置、移除托盘 → `CubismModel::shutdown`。
- 单实例 Mutex 在 `Drop` 时释放。

## 10. Config schema

```json
{
  "window": { "x": 200, "y": 200, "width": 400, "height": 400 },
  "character": { "active_character": "default" },
  "auto_start": false,
  "visible": true
}
```

所有字段 `#[serde(default)]`；缺失自动补默认值。

## 11. 已知技术债

- shim 只在首次 `cargo build` 时编译；改了 `shim.cpp` 需要手动 `cmake --build shim/build --config Release`。
- 纹理 premultiply 在 CPU 上做（一次性，不影响运行）。
- 气泡定位基于窗口矩形（非可见几何包围盒）。
- 单显示器 DPI 场景已测试；多显示器混合 DPI 未深入测试。
- 无自动更新、无安装器（绿色解压运行）。

## 12. 未纳入（明确延后）

- AI / LLM / TTS / Memory / Persona（已从 15 阶段版本删除，见 tag `desktop-pet-ai-archive`）
- 多角色包自动下载 / 商店
- 网络功能
- 自动更新

## 13. 版本

- v0.1.0 — 纯 DesktopPet 首发版本，从 AI Companion 版本收束。
- 上一个 AI 完整版在 tag `desktop-pet-ai-archive`（含 pet-agent、Memory、Persona、TTS、Piper、AI UI）。
