# DesktopPet — Live2D 桌宠 Runtime

> 本文件记录项目现状、架构决策、本轮修改与技术债。
> 由 Cuckoo Code 生成/维护。

## 1. 项目定位

Windows 桌宠 Runtime。Rust 应用壳 + C++ Cubism shim 混合架构，
从零构建，不复用任何旧桌宠工程。

原始 Live2D 资产（`C:\AI\QwenGame\test-psd2live`）只作为**只读输入**，
运行时所需资产已复制到项目自有的 `characters/default/` 角色包中。
Release 程序连同 `characters/` 目录可以整体移动。

## 2. 技术选型与架构

### 2.1 关键约束

- 本机仅有 **VS 2022 BuildTools**，`cl.exe` 需 vcvars。
- Windows SDK 10.0.26100。
- Cubism Native SDK 的 **Core 为预编译 C 静态库**，**Framework 为 C++ 源码**。
- 依赖 DirectXMath（`C:/AI/QwenGame/dxmath_extract/include`）。

### 2.2 架构决策

**Rust 应用壳 + C++ Cubism 静态库 shim：**

- **Rust 侧**：Win32 透明置顶窗口、DirectComposition、D3D11、消息循环、
  鼠标拖拽、命中测试、事件采集、Behavior、配置持久化、角色包管理。
- **C++ 侧**：编译 Cubism Native Framework + Core，暴露薄 `extern "C"` 接口，
  负责模型解析、物理、动作、绘制、几何命中测试、视线参数写入。
  **不含任何 Win32 窗口逻辑**。

### 2.3 目录结构

```
DesktopPet/
├── Cargo.toml
├── build.rs                # 触发 CMake 构建 shim，链接静态库，部署 shader 和 characters
├── src/
│   ├── main.rs             # 入口（windows_subsystem = "windows"）
│   ├── app.rs              # 生命周期 + 主循环 + 事件消费 + 视线跟随 + Reset/Quit
│   ├── config.rs           # 配置读写 + 屏幕越界修正 + log_line
│   ├── behavior/
│   │   └── mod.rs          # PetEvent / PetAction / BehaviorController
│   ├── character/
│   │   ├── mod.rs          # 模块导出
│   │   ├── package.rs      # character.json 解析 + 验证 + 语义动作映射
│   │   ├── manager.rs      # CharacterManager：扫描 / 提供角色包
│   │   └── cubism.rs       # C++ shim 的 FFI 安全封装
│   └── platform/
│       ├── mod.rs
│       ├── window.rs       # Win32 窗口 + 命中测试 + 拖拽 + 右键菜单 + 事件采集
│       └── gfx.rs          # D3D11 + DirectComposition 合成交换链
├── characters/             # 角色包（随程序分发）
│   └── default/
│       ├── character.json
│       └── model/          # Cubism 资产
├── shim/                   # C++ Cubism 封装（独立 CMake 工程）
│   ├── CMakeLists.txt
│   ├── shim.cpp
│   └── thirdparty/stb_image.h
└── vendor/CubismSdkForNative-5-r.5/   # SDK（gitignored）
```

### 2.4 渲染管线

```
Win32 窗口 (WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW)
  → DirectComposition 视觉树
    → DXGI Flip SwapChain (DXGI_ALPHA_MODE_PREMULTIPLIED, B8G8R8A8)
      → D3D11 渲染目标（每帧绑定 + 透明清屏）
        → Cubism D3D11 Renderer 绘制 Live2D 模型
```

## 3. Character Package 系统

### 3.1 目录规范

```
characters/
└── <id>/
    ├── character.json          # 必需：角色元数据
    └── model/                  # 约定目录（可由 character.json 指向任意相对路径）
        ├── <name>.model3.json
        ├── <name>.moc3
        ├── <name>.physics3.json
        ├── <name>.cdi3.json
        ├── <name>.motion3.json ...
        └── texture...
```

### 3.2 character.json schema

```json
{
  "id": "default",                    // 必需，应与目录名一致
  "display_name": "Default Pet",     // 可选，UI 显示名
  "model": {
    "model3": "model/test.model3.json",  // 必需，相对角色根目录
    "scale": 1.0,                     // 可选，默认 1.0，范围 [0.1, 5.0]
    "offset_x": 0.0,                  // 可选，canvas 单位
    "offset_y": 0.0
  },
  "motions": {                        // 可选，语义动作 → Cubism motion group
    "Idle": "Idle",
    "Blink": "Blink",
    "Nod": "Nod",
    "Shake": "Shake"
  },
  "behavior": {                       // 可选
    "double_click_ms": 400,
    "drag_threshold_px": 5
  }
}
```

**语义动作映射规则**：
- Runtime 只认识 Idle / Blink / Nod / Shake 四个语义动作。
- character.json 的 motions 把它们映射到角色实际的 motion group 名。
- 缺失映射时回退为「与语义同名」。
- 若角色没有某个 group，`start_motion` 返回失败但不崩溃。

### 3.3 CharacterManager

- `CharacterManager::scan(root)`：扫描 characters/ 下所有子目录。
  - 对每个子目录尝试 `CharacterPackage::load`。
  - **坏角色包仅记录日志并跳过，不中断扫描、不 panic。**
  - 扫描只在启动（或显式刷新）时执行，不每帧扫描。
- `resolve_active(id)`：按 id 选择激活角色；找不到时 fallback 到第一个。

### 3.4 角色加载流程

```
Config (active_character="default")
  → CharacterManager::scan("characters/")
  → CharacterPackage::load("characters/default/")
      ├─ 解析 character.json
      ├─ 验证 model3 存在（相对路径，禁止绝对路径）
      ├─ clamp scale 到 [0.1, 5.0]
      └─ 通过
  → CharacterManager::resolve_active("default")
  → CubismModel::load(model3_path, scale, offset)
      → shim: 应用 scale/offset 到模型矩阵
```

## 4. 已实现的功能

### 4.1 底层（第一 / 第二阶段）
- [x] Rust + C++ 混合工程，透明置顶窗口，D3D11 + DirectComposition
- [x] Live2D 模型加载、物理、纹理、动作
- [x] 60 FPS 主循环（sleep 限帧，无忙循环）
- [x] 基于 Cubism drawable 几何的点击穿透（Hit Test）
- [x] 拖拽移动（5px 阈值区分点击/拖拽），位置保存 / 恢复 / 越界修正
- [x] Behavior 层（PetEvent → PetAction → Cubism motion）
- [x] 单击 → Nod，双击 → Shake

### 4.2 第三阶段（本轮）
- [x] **Character Package 系统**：`characters/<id>/` 目录规范 + `character.json`
- [x] **CharacterManager**：扫描、解析、验证、容错（坏包跳过）
- [x] **移除原始绝对路径依赖**：运行时使用 `characters/default/`（或 exe 同目录），
      `C:\AI\QwenGame\test-psd2live` 不再被引用
- [x] **Config 迁移**：`asset_dir/model_name` → `active_character` 名称
- [x] **视线跟随**：鼠标位置（每帧 GetCursorPos）→ ParamAngleX/Y + ParamEyeBallX/Y，
      指数平滑插值；仅在 Idle 优先级时写入，不干扰 Nod/Shake
- [x] **右键菜单**：命中角色才弹出，含 点头 / 摇头 / 重置位置 / 退出
- [x] **Reset Position**：`SPI_GETWORKAREA` 获取工作区（不含任务栏），移到右下角
- [x] **Scale 迁移**：从 character.json 读取并 clamp，应用到 Cubism 模型矩阵
- [x] **build.rs 部署 characters/**：连同 exe 一起可移动

## 5. 构建与运行

```powershell
cargo build
cargo build --release

cd target\debug
.\desktop-pet.exe
```

- `build.rs` 首次构建自动调用 CMake 编译 shim，并部署 `FrameworkShaders/*.fx` 和 `characters/`。
- **前置依赖**：VS 2022 (C++ 工具链)、CMake、Rust (windows-msvc toolchain)。
- **运行时日志**：`log_line()` 同时写 stderr 与 exe 同目录 `pet_runtime.log`。

### 5.1 交互操作

| 操作 | 行为 |
|---|---|
| 单击角色 | 播放 Nod 动作 |
| 双击角色（400ms 内） | 播放 Shake 动作 |
| 按住角色拖动 | 移动窗口；结束/关闭时保存位置 |
| 鼠标靠近 / 移动 | 视线跟随（头部 + 眼球） |
| 右键角色 | 弹出菜单：点头 / 摇头 / 重置位置 / 退出 |
| 透明区域 | 鼠标穿透到后方窗口 |
| ESC | 退出 |

## 6. 调试期间修复的关键问题

1. **MSVC UTF-8 编码**：Cubism SDK 中文注释 → 添加 `/utf-8`。
2. **CubismFramework::Option 生命周期**：改为全局静态 `g_option`。
3. **渲染目标未绑定**：每帧显式绑定 + 设置视口。
4. **windows 0.62 API 适配**：HWND 空指针、三参数 CreateRenderTargetView、bool CreateTargetForHwnd、
   `Some(0)` 用于 TrackPopupMenu 的 nreserved、`SPI_GETWORKAREA` 常量。
5. **C++/Rust stderr 交错丢行**：log_line 增加独立文件输出。
6. **混合行尾（LF/CRLF）导致 edit 匹配失败**：用 readLines + write 重写。

## 7. 已知技术债 / 待改进

- **动作调度**：BehaviorController 只缓存单个 pending 动作（优先级覆盖），
  无队列 / 冷却；连续快速点击可能被高优先级覆盖。
- **空闲行为**：IdleTimeout 事件未实现；blink 由 shim 内部 idle 循环驱动。
- **右键菜单选择未自动化**：原生 TrackPopupMenu 的菜单项选择难以自动化测试，
  人工验证可用；进程不崩溃。
- **Look 参数写入策略**：直接写 ParamAngleX/Y + ParamEyeBallX/Y，
  未使用 `CubismLook` 组件；未处理模型缺少参数的情况（SetParameterValue 对未知 ID 为 no-op）。
- **Hit Test 精度**：三角形几何命中，非纹理像素级 alpha。
- **设备丢失处理**：未处理 `DXGI_ERROR_DEVICE_REMOVED`。
- **多角色 UI**：无角色选择界面（架构已就绪）。
- **shim 加载期日志**：保留若干 `fprintf(stderr)`（非每帧）。

## 8. 下一阶段建议（优先级）

1. **角色切换 UI**（托盘/菜单）：利用 CharacterManager::list 枚举角色。
2. **行为丰富化**：空闲随机 blink/nod；BehaviorController 升级为队列 + 冷却。
3. **视线跟随增强**：使用 CubismLook 组件；支持只写存在的参数。
4. **系统托盘 + 开机启动**。
5. **设备丢失处理**：重建交换链与资源。

## 9. 未纳入本轮（明确延后）

- Live2D 之外的渲染后端、文字气泡 / UI
- AI / Persona / Memory / TTS / ASR
- 自动走路 / 多显示器跨屏、情绪系统、在线角色下载、插件系统、自动更新

---

## 10. 第三阶段实现细节（本轮）

### 10.1 Character 加载与容错

- `CharacterPackage::load` 逐一验证：目录存在 → character.json 可读可解析 →
  id 非空 → model3 为相对路径且存在 → scale 有限并 clamp。
- 任一验证失败返回 `Err(String)`；`CharacterManager::scan` 捕获后记录日志并继续。
- 因此一个损坏角色包不会影响其他角色被发现。

### 10.2 Look Tracking 参数与更新流程

**使用的 Cubism 参数**（来自 test.cdi3.json）：
- `ParamAngleX` / `ParamAngleY`：头部角度，范围 ±30
- `ParamEyeBallX` / `ParamEyeBallY`：眼球位置，范围 ±1

**更新顺序**（在 shim Update 内，与 Cubism 推荐一致）：
```
motion (Idle/Blink/Nod/Shake) 播放
  → 若 motion priority <= 1（即 Idle）则写入 look 参数
  → physics evaluate（物理带动头发等）
  → model save + update
```

**平滑**：指数插值 `current += (target - current) * (1 - exp(-10*dt))`，与帧率无关。

### 10.3 高频鼠标位置处理

**不使用 mpsc 排队鼠标坐标。** 视线目标每帧由主循环通过一次 `GetCursorPos()` 查询，
计算鼠标相对窗口的归一化坐标后直接调用 `set_look`。这样：
- 无队列积压、无内存增长；
- 每帧仅一次系统调用；
- 鼠标离开 LOOK_RADIUS 倍窗口范围时目标归零，视线平滑回中。

离散事件（click / enter / leave / drag）仍走 mpsc::channel。

### 10.4 右键菜单事件流

```
WM_RBUTTONUP (wndproc)
  → 命中测试：命中才弹菜单，否则忽略（穿透）
  → send(PetEvent::RightClick)
  → TrackPopupMenu (原生菜单，阻塞返回命令 ID)
  → 根据 ID send(PetEvent::MenuNod / MenuShake / MenuReset / MenuQuit)
  → app 主循环 pump_behavior
      ├─ Nod/Shake → CubismModel::start_motion
      ├─ ResetPosition → SPI_GETWORKAREA + SetWindowPos + save
      └─ Quit → DestroyWindow
```

### 10.5 Reset Position

- `SystemParametersInfoW(SPI_GETWORKAREA)` 获取主显示器工作区（排除任务栏）。
- 移动窗口到工作区右下角，留 20px 边距。
- 不硬编码分辨率，跨分辨率自适应。
- 移动后由既有 WM_DESTROY/拖拽逻辑或显式调用保存配置。
```
