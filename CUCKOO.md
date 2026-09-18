# DesktopPet — Live2D 桌宠 Runtime

> 本文件记录项目现状、架构决策、本轮修改与技术债。
> 由 Cuckoo Code 生成/维护。

## 1. 项目定位

以 `C:\AI\QwenGame\test-psd2live` 中已生成的 PSD2Live / Live2D 角色资产为**只读输入**，
从零构建一个 Windows 桌宠 Runtime。**不复用、不迁移任何旧桌宠工程**。

## 2. 输入资产审计（只读）

资产目录：`C:\AI\QwenGame\test-psd2live`

| 文件 | 大小 | 说明 |
|---|---|---|
| `test.model3.json` | 709 B | 模型定义，Version 3 |
| `test.moc3` | 117 KB | Cubism 5 编译模型 |
| `test.cdi3.json` | 4 KB | 参数/部件/绘制信息 |
| `test.physics3.json` | 8.5 KB | 物理 |
| `test.idle/blink/nod/shake.motion3.json` | 1–2 KB | 4 组动作 |
| `test.2048/texture_00.png` | — | 2048 图集 |
| `test.cmo3` | 10.9 MB | Cubism 工程文件（运行时不需要） |
| `test.psd2live.json` | 8 KB | PSD2Live 生成元数据 |

**引用完整性**：`model3.json` 引用的 Moc / Textures / Physics / DisplayInfo / Motions 全部存在，**资产完整可用**。

**版本兼容性**：`runtimeTarget = Cubism50`, `mocVersion = 5`，与所用 **Cubism SDK for Native 5-r.5** 完全匹配。

## 3. 技术选型与架构

### 3.1 关键约束（实测）

- 本机仅有 **VS 2022 BuildTools**，`cl.exe` 不在 PATH（需 vcvars）。
- Windows SDK 10.0.26100。
- Cubism Native SDK 的 **Core 为预编译 C 静态库/DLL**，**Framework 为 C++ 源码**（moc3 解析、物理、动作、D3D11 渲染器）。
- 本机已有 `directxtk` / `directxmath` 依赖，但 Framework 实际只依赖 **DirectXMath**（DirectXTK 仅 Demo 用）。

### 3.2 架构决策

**采用 Rust 应用壳 + C++ Cubism 静态库 shim 的混合架构：**

- **Rust 侧**（主体）：Win32 透明置顶窗口、DirectComposition 合成、D3D11 设备/交换链、消息循环、鼠标拖拽、配置持久化。
- **C++ 侧**：编译 Cubism Native Framework + Core，暴露薄 `extern "C"` 接口，仅负责模型解析与绘制，**不含任何 Win32 窗口逻辑**。

理由：用纯 Rust 重写 Cubism 的 moc3 解析 / 物理引擎 / 动作系统成本极高且风险大；用 C++ 静态库隔离既复用官方 SDK，又保持应用层为 Rust + Win32，界面清晰。

### 3.3 目录结构

```
DesktopPet/
├── Cargo.toml
├── build.rs                # 触发 CMake 构建 shim，链接静态库，部署 shader
├── src/
│   ├── main.rs             # 入口（windows_subsystem = "windows"）
│   ├── app.rs              # 应用生命周期 + 主循环 + 事件消费
│   ├── config.rs           # 配置读写 + 屏幕越界修正 + log_line
│   ├── behavior/
│   │   └── mod.rs          # PetEvent / PetAction / BehaviorController
│   ├── character/
│   │   ├── mod.rs          # 角色资产抽象 + 语义动作映射
│   │   └── cubism.rs       # C++ shim 的 FFI 安全封装
│   └── platform/
│       ├── mod.rs
│       ├── window.rs       # Win32 透明置顶窗口 + 命中测试 + 拖拽 + 事件采集
│       └── gfx.rs          # D3D11 + DirectComposition 合成交换链
├── shim/                   # C++ Cubism 封装（独立 CMake 工程）
│   ├── CMakeLists.txt
│   ├── shim.cpp            # FFI 实现（模型加载/更新/绘制/动作/命中测试）
│   └── thirdparty/stb_image.h
└── vendor/CubismSdkForNative-5-r.5/   # SDK（解压自 zip，gitignored）
```

### 3.4 渲染管线

```
Win32 窗口 (WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW)
  → DirectComposition 视觉树
    → DXGI Flip SwapChain (DXGI_ALPHA_MODE_PREMULTIPLIED, B8G8R8A8)
      → D3D11 渲染目标（每帧绑定 + 透明清屏）
        → Cubism D3D11 Renderer 绘制 Live2D 模型
```

## 4. 已实现的功能

### 4.1 第一阶段（已完成的底层）
- [x] 从零建立 Rust + C++ 混合工程
- [x] 编译 Cubism Native Framework（D3D11 后端）+ Core
- [x] 透明、无边框、置顶、逐像素透明的桌面窗口
- [x] 正确加载 `test.model3.json` 及全部引用资产
- [x] 正确渲染 Live2D 模型（含物理、纹理）
- [x] 稳定帧循环（60 FPS 目标，sleep 限帧，无忙循环）
- [x] 配置首次运行自动写出

### 4.2 第二阶段（本轮：交互与 Behavior）
- [x] **点击穿透 / Hit Test**：C++ shim 用 Cubism drawable 几何（顶点 + 三角形 + opacity + visible flag）
      与缓存的 MVP 矩阵反算进行命中测试；WM_NCHITTEST 中命中返回 HTCLIENT，
      透明区域返回 HTTRANSPARENT（穿透到后方窗口）
- [x] **拖拽完善**：只有命中角色本体才能开始拖动；透明区域不拦截；
      使用 5px 位移阈值区分「点击」与「拖拽」；拖拽结束/关闭时保存位置
- [x] **窗口位置越界修正**：加载 config 时如果坐标在虚拟屏幕外，自动 clamp 到可见区域
- [x] **Behavior 层**：新建 `src/behavior/mod.rs`，定义 PetEvent / PetAction / BehaviorController；
      窗口层只采集输入事件，Behavior 决定语义动作
- [x] **语义动作映射**：`CharacterAssets::action_group(name)` 将 Idle/Blink/Nod/Shake 映射到 model3.json 中的 motion group，
      换角色时只需改映射
- [x] **最小动作调度器**：BehaviorController 缓存一个 pending 动作，高优先级可覆盖低优先级；
      主循环消费后触发 motion；priority 1=Idle / 2=Nod / 3=Shake
- [x] **交互→动作映射**：单击 → Nod，双击（400ms 内）→ Shake；拖拽期间不触发其它动作
- [x] **事件通道**：window wndproc → mpsc::channel → app 主循环消费，解耦输入与行为

## 5. 构建与运行

```powershell
# Debug / Release 构建
cargo build
cargo build --release

# 运行（工作目录需为 exe 所在目录：FrameworkShaders 通过相对路径加载）
cd target\debug
.\desktop-pet.exe
```

- `build.rs` 会在首次构建时自动调用 CMake 配置/编译 shim，并部署 `FrameworkShaders/*.fx` 到输出目录。
- **前置依赖**：VS 2022 (C++ 工具链)、CMake、Rust (windows-msvc toolchain)。
- **运行时日志**：`log_line()` 同时写 stderr 和 exe 同目录的 `pet_runtime.log`，
  便于在与 C++ shim 的 stderr 交错时排查（C++ shim 的 `fprintf(stderr)` 会与 Rust 的 `eprintln!` 竞争）。

### 5.1 交互操作

| 操作 | 行为 |
|---|---|
| 单击角色 | 播放 Nod 动作 |
| 双击角色（400ms 内） | 播放 Shake 动作 |
| 按住角色拖动 | 移动窗口；结束/关闭时保存位置 |
| 透明区域 | 鼠标穿透到后方窗口 |
| ESC | 退出 |

## 6. 调试期间修复的关键问题

1. **MSVC UTF-8 编码**：Cubism SDK 源码含中文注释，MSVC 默认按 GBK 读取导致行连接吞代码 → 添加 `/utf-8` 编译选项。
2. **CubismFramework::Option 生命周期**：`StartUp` 内部保存的是 Option **指针**，栈上局部变量导致悬垂，`GetLoadFileFunction()` 读到垃圾指针崩溃 → 改为全局静态 `g_option`。
3. **渲染目标未绑定**：Rust 路径遗漏 `OMSetRenderTargets`，导致模型画到无效目标 → 每帧显式绑定交换链后备缓冲 + 设置视口。
4. **windows 0.62 API 适配**：`HWND` 为空指针表示、`CreateRenderTargetView` 三参数、`CreateTargetForHwnd` 用 bool 等。

## 7. 已知技术债 / 待改进

- **右键菜单**：暂以 ESC 退出，尚未实现桌宠右键菜单（切换动作、退出等）。PetEvent::RightClick 已预留。
- **开机启动**：未实现。
- **动作调度**：BehaviorController 目前只缓存单个 pending 动作（优先级覆盖），未实现队列/冷却时间；
  连续快速点击仍可能被更高优先级覆盖。
- **视线跟随**：未接入鼠标位置驱动 `CubismLook`；`PetEvent::PointerEnter/Leave/Dragging` 已预留。
- **空闲超时**：`IdleTimeout` 事件未实现；blink 目前由 shim 内部的 idle 循环驱动。
- **Hit Test 精度**：目前是三角形几何命中，不做纹理像素级 alpha（符合第二阶段要求，足够区分角色/透明背景）。
- **单一模型路径**：角色资产目录写死在配置默认值，尚未实现 `characters/<name>/` 角色包扫描。
- **设备丢失处理**：未处理 `DXGI_ERROR_DEVICE_REMOVED`。
- **Shim 诊断日志**：shim.cpp 仍含若干 `fprintf(stderr)` 加载期日志（非每帧），保留作为加载期诊断。

## 8. 下一阶段建议（优先级）

1. **右键菜单**：切换动作、退出、设置。
2. **行为丰富化**：PointerEnter/Leave 触发视线跟随；空闲随机 blink/nod；
   BehaviorController 从「单 pending」升级为「队列 + 冷却」。
3. **视线跟随**：鼠标位置 → 头部/眼球参数（CubismLook 或直接 SetParameterValue）。
4. **开机启动 + 系统托盘**。
5. **角色包系统**：`characters/<name>/` 约定 + 配置切换。
6. **设备丢失处理**：`DXGI_ERROR_DEVICE_REMOVED` 时重建交换链。

## 9. 未纳入本轮（明确延后）

- Live2D 之外的渲染后端（Sprite Renderer 等）
- 文字气泡 / UI
- AI / Persona 模块
- 表情（`.cmo3` 内嵌表情未导出为 `exp3.json`，暂不支持切换表情）
- TTS / ASR / 多角色 UI / 自动走路 / 情绪系统 / 网络功能

---

## 10. 第二阶段实现细节（本轮）

### 10.1 命中测试原理

C++ shim 在 `Draw` 时缓存当帧 MVP 矩阵，`HitTest(winX, winY, winW, winH)`：
1. 窗口像素坐标 → NDC（Y 翻转；Cubism 画布 Y 向上）：`ndcX = 2*x/w - 1`, `ndcY = 1 - 2*y/h`
2. MVP 逆矩阵 → canvas 坐标（CubismMatrix44 为列主序）
3. 遍历所有 drawable：
   - 跳过 `!GetDrawableDynamicFlagIsVisible` 或 `opacity <= 0.001`
   - 遍历三角形索引，用重心坐标法判断点是否落在三角形内
4. 任一三角形命中即返回 true

**性能**：每次 hit test 只做矩阵逆变换 + 三角形遍历，无 GPU readback、无内存分配、无文件 IO。
模型 23 个 drawable，每帧最坏几十次测试完全可接受。

### 10.2 事件流

```
Win32 消息 (wndproc)
  ↓ WM_NCHITTEST 判定是否命中 → HTCLIENT / HTTRANSPARENT
  ↓ WM_MOUSEMOVE / WM_LBUTTONDOWN / WM_LBUTTONUP / WM_RBUTTONUP
PetEvent (PointerEnter / LeftClick / DoubleClick / DragStart / DragEnd / ...)
  ↓ mpsc::channel
app.rs 主循环 pump_behavior
  ↓ BehaviorController::handle
PetAction (Idle / Blink / Nod / Shake) + priority
  ↓ take_pending
CharacterAssets::action_group(name) → motion group
  ↓ CubismModel::start_motion
shim: CubismMotionManager::StartMotionPriority
```

### 10.3 关键常量

- `DRAG_THRESHOLD = 5` px：超过视为拖拽
- `DOUBLE_CLICK_MS = 400` ms：双击判定窗口
- `MARGIN = 100` px：位置修正时至少保留的可见像素
- Priority: 1=Idle, 2=Nod, 3=Shake
