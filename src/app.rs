//! 应用生命周期与主循环。
//!
//! 流程：Config → CharacterManager → CharacterPackage → Cubism model。
//! 每帧：泵消息 → 消费事件 → Behavior → 动作 → 渲染。
//! 视线跟随：每帧查询鼠标位置（GetCursorPos，一次系统调用），归一化后传给 shim，
//! 不使用 mpsc 排队高频鼠标坐标。

use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::behavior::{BehaviorController, PetAction, PetEvent};
use crate::character::cubism::CubismModel;
use crate::character::CharacterManager;
use crate::config::{log_line, Config};
use crate::platform::gfx::Gfx;
use crate::platform::window::{create_window, set_model, WindowParams};

/// 视线跟随参数。
/// 鼠标超出窗口尺寸的 LOOK_RADIUS 倍时视为「离开」，目标归零。
const LOOK_RADIUS: f32 = 1.5;

pub struct App {
    config: Config,
    events_rx: Option<Receiver<PetEvent>>,
    behavior: BehaviorController,
}

impl App {
    pub fn new() -> Self {
        App {
            config: Config::load(),
            events_rx: None,
            behavior: BehaviorController::new(),
        }
    }

    pub fn run(&mut self) -> windows::core::Result<()> {
        let wc = &self.config.window;
        let (win_w, win_h) = (wc.width, wc.height);
        let params = WindowParams {
            x: wc.x,
            y: wc.y,
            width: wc.width,
            height: wc.height,
        };

        // 扫描角色包（仅在启动时执行）
        let characters_root = Self::characters_root();
        let manager = CharacterManager::scan(&characters_root);
        let active = manager.resolve_active(&self.config.character.active_character);

        unsafe {
            let (tx, rx) = channel::<PetEvent>();
            self.events_rx = Some(rx);

            let hwnd: HWND = create_window(&params, self.config.clone(), tx)?;
            let gfx = Gfx::new(hwnd, win_w, win_h)?;

            let model = match active {
                Some(pkg) => Self::init_model(&gfx, pkg, win_w, win_h),
                None => {
                    log_line("no character available");
                    None
                }
            };

            if let Some(m) = &model {
                set_model(hwnd, m.raw_handle());
            }

            let frame_target = std::time::Duration::from_micros(16_667);
            let mut last = Instant::now();
            let mut msg = MSG::default();

            'running: loop {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break 'running;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                let now = Instant::now();
                let dt = now.duration_since(last).as_secs_f32();
                last = now;

                // 处理事件 → 行为 → 动作
                if let Some(action) = self.pump_behavior(&model, active) {
                    if action == PetAction::Quit {
                        let _ = DestroyWindow(hwnd);
                    }
                }

                // 视线跟随（每帧查询鼠标位置，一次系统调用）
                if let Some(m) = &model {
                    if let Some((lx, ly)) = Self::compute_look(hwnd, win_w, win_h) {
                        m.set_look(lx, ly);
                    } else {
                        m.set_look(0.0, 0.0);
                    }
                }

                gfx.bind_render_target(win_w, win_h);
                gfx.clear_transparent();
                if let Some(m) = &model {
                    m.update(dt);
                    m.draw(win_w as f32, win_h as f32);
                }
                gfx.present()?;

                let elapsed = now.elapsed();
                if elapsed < frame_target {
                    std::thread::sleep(frame_target - elapsed);
                }
            }

            if model.is_some() {
                CubismModel::shutdown();
            }
            log_line("main loop exited");
        }
        Ok(())
    }

    /// characters/ 目录：exe 同目录优先，其次项目根（开发时）。
    fn characters_root() -> std::path::PathBuf {
        let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
        p.pop();
        let exe_dir = p.clone();
        let exe_chars = exe_dir.join("characters");
        if exe_chars.is_dir() {
            return exe_chars;
        }
        // 开发回退：CARGO_MANIFEST_DIR/characters
        let manifest_chars =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("characters");
        if manifest_chars.is_dir() {
            return manifest_chars;
        }
        exe_chars
    }

    /// 处理事件队列与待触发动作。返回触发的最顶层动作（若有）。
    fn pump_behavior(
        &mut self,
        model: &Option<CubismModel>,
        active: Option<&crate::character::CharacterPackage>,
    ) -> Option<PetAction> {
        if let Some(rx) = &self.events_rx {
            while let Ok(ev) = rx.try_recv() {
                self.behavior.handle(ev);
            }
        }

        let (action, priority) = self.behavior.take_pending()?;

        match action {
            PetAction::ResetPosition => {
                Self::reset_position();
                return Some(action);
            }
            PetAction::Quit => {
                log_line("quit requested from menu");
                return Some(action);
            }
            _ => {}
        }

        if let (Some(m), Some(pkg)) = (model, active) {
            if let Some(group) = pkg.action_group(action.name()) {
                let ok = m.start_motion(&group, 0, priority);
                log_line(&format!(
                    "action: {:?} -> group={} priority={} ok={}",
                    action, group, priority, ok
                ));
            } else {
                log_line(&format!(
                    "action: {:?} unavailable for this character",
                    action
                ));
            }
        }
        Some(action)
    }

    /// 将窗口移回工作区右下角（不遮挡任务栏）。
    fn reset_position() {
        unsafe {
            let mut wa = RECT::default();
            let ok = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut wa as *mut _ as *mut std::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            if ok.is_err() {
                log_line("reset position: SPI_GETWORKAREA failed");
                return;
            }
            // 找到自己的 hwnd —— 通过 FindWindow 类名
            let hwnd = FindWindowW(windows::core::w!("DesktopPetWindow"), None).unwrap_or_default();
            if hwnd.0.is_null() {
                return;
            }
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            let margin = 20;
            let nx = wa.right - w - margin;
            let ny = wa.bottom - h - margin;
            let _ = SetWindowPos(
                hwnd,
                None,
                nx,
                ny,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            log_line(&format!("reset position to ({nx},{ny})"));
        }
    }

    /// 计算视线归一化坐标（-1..1）。
    /// 返回 None 表示鼠标距窗口太远（应回中）。
    fn compute_look(hwnd: HWND, win_w: i32, win_h: i32) -> Option<(f32, f32)> {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);

            // 鼠标相对窗口的像素坐标
            let rx = pt.x - rect.left;
            let ry = pt.y - rect.top;

            // 超出 LOOK_RADIUS 倍窗口尺寸 → 认为远离
            let max_x = (win_w as f32) * LOOK_RADIUS;
            let max_y = (win_h as f32) * LOOK_RADIUS;
            if (rx as f32) < -max_x
                || (rx as f32) > max_x
                || (ry as f32) < -max_y
                || (ry as f32) > max_y
            {
                return None;
            }

            // 归一化到 [-1, 1]（窗口中心为 0），并 clamp
            let nx = (rx as f32 / win_w as f32) * 2.0 - 1.0;
            let ny = 1.0 - (ry as f32 / win_h as f32) * 2.0; // Y 翻转
            Some((nx.clamp(-1.0, 1.0), ny.clamp(-1.0, 1.0)))
        }
    }

    unsafe fn init_model(
        gfx: &Gfx,
        pkg: &crate::character::CharacterPackage,
        width: i32,
        height: i32,
    ) -> Option<CubismModel> {
        log_line("step: cubism startup");
        if !CubismModel::startup() {
            log_line("cubism startup failed");
            return None;
        }
        log_line("step: set device");
        CubismModel::set_device(gfx.device_ptr(), gfx.context_ptr());

        let model3 = pkg.model3.to_string_lossy().to_string();
        log_line(&format!(
            "step: load model (id={} scale={})",
            pkg.id(),
            pkg.scale
        ));
        let model = CubismModel::load(&model3, width, height, pkg.scale, pkg.offset());
        if model.is_some() {
            log_line("live2d model ready");
        }
        model
    }
}
