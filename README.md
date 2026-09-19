# DesktopPet

一个轻量的 Windows 桌宠：Live2D 模型 + 透明窗口 + D3D11 渲染。双击即可运行，常驻桌面。

## 运行要求

- Windows 10 / 11 (x64)
- 支持 DirectX 11 的显卡
- 无需安装：解压后直接双击 `DesktopPet.exe`

## 快速上手

1. 双击 `DesktopPet.exe`
2. 桌宠出现在桌面右下角附近
3. 拖动角色可移动位置（自动保存）
4. 右键角色弹出菜单
5. 托盘图标（右下角通知区）提供完整菜单

## 操作

| 操作 | 行为 |
|---|---|
| 单击角色 | 点头 |
| 双击角色 | 摇头 |
| 拖动角色 | 移动窗口（松开后保存位置） |
| 鼠标靠近 | 视线跟随 |
| 右键角色 | 弹出菜单（点头 / 摇头 / 重置位置 / 退出） |
| 左键托盘图标 | 显示 / 隐藏 |
| 右键托盘图标 | 显示隐藏 / 角色 / 测试气泡 / 重置位置 / 开机启动 / 退出 |
| ESC | 退出 |

## 托盘菜单

- **显示 / 隐藏桌宠**：控制窗口可见性（进程保留）
- **角色**：多角色包时切换当前角色
- **测试气泡**：显示一条测试文字气泡
- **重置位置**：将角色移到屏幕右下角
- **开机启动**：勾选后随 Windows 启动（写入 HKCU\Software\Microsoft\Windows\CurrentVersion\Run）
- **退出**：结束进程

## 文件结构

```
DesktopPet/
├── DesktopPet.exe            # 主程序
├── characters/               # 角色包目录
│   └── default/
│       ├── character.json    # 角色元数据
│       └── model/...         # Live2D 模型文件（moc3 / 动作 / 纹理）
├── FrameworkShaders/         # Cubism D3D11 shader
├── desktop-pet.config.json   # 运行时生成：窗口位置、角色、开机启动
├── pet_runtime.log           # 运行时日志（每次启动截断）
└── README.md
```

## 添加新角色

1. 在 `characters/` 下建立新目录，如 `characters/mychar/`
2. 放入 Live2D 模型文件（Cubism 3+ 导出的 `.model3.json` + `.moc3` + 纹理 + 动作）
3. 创建 `character.json`：

```json
{
  "id": "mychar",
  "display_name": "My Character",
  "model": {
    "model3": "model/mychar.model3.json",
    "scale": 0.75,
    "offset_x": 0.0,
    "offset_y": 0.0
  },
  "motions": {
    "Idle": "Idle",
    "Blink": "Blink",
    "Nod": "Nod",
    "Shake": "Shake"
  }
}
```

4. 通过托盘菜单「角色」子菜单切换，或编辑 `desktop-pet.config.json` 的 `active_character`

坏角色包（缺少文件 / JSON 损坏）只记日志，不会阻止程序启动。

## 卸载

- 运行托盘菜单「退出」结束进程
- 若启用了开机启动，先在托盘菜单取消勾选，或删除注册表项 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\DesktopPet`
- 删除整个 `DesktopPet` 目录

## 许可
MIT License

Copyright (c) 2026 VeiQiuLab

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## License

本项目自有代码采用 [MIT License](LICENSE)。

Live2D Cubism SDK、默认角色资源及其他第三方内容不属于本 MIT License 授权范围，其使用需遵守各自的许可条款。
