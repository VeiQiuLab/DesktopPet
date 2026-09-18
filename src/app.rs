//! 应用生命周期与主循环。
//!
//! 里程碑1：透明置顶窗口 + D3D11/DComp 交换链。
//! 里程碑2：加载 Live2D 模型并每帧渲染。
//! 里程碑3：motion 播放 / 拖拽 / 位置持久化。
//! 里程碑4：点击穿透 + Behavior 层 + 动作交互。

use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::behavior::{BehaviorController, PetEvent};
use crate::character::cubism::CubismModel;
use crate::character::CharacterAssets;
use crate::config::{log_line, Config};
use crate::platform::gfx::Gfx;
use crate::platform::window::{create_window, set_model, WindowParams};

pub struct App {
    config: Config,
    /// 事件接收端（由窗口层发送）。
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

        unsafe {
            let (tx, rx) = channel::<PetEvent>();
            self.events_rx = Some(rx);

            let hwnd: HWND = create_window(&params, self.config.clone(), tx)?;
            let gfx = Gfx::new(hwnd, win_w, win_h)?;

            let assets = CharacterAssets::from_config(&self.config.character);
            let model = Self::init_model(&gfx, &assets, win_w, win_h);

            // 挂上模型句柄用于命中测试
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

                // 消费事件 → Behavior → 动作
                self.pump_behavior(&model, &assets);

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

    /// 处理事件队列与待触发动作。
    fn pump_behavior(&mut self, model: &Option<CubismModel>, assets: &CharacterAssets) {
        if let Some(rx) = &self.events_rx {
            while let Ok(ev) = rx.try_recv() {
                self.behavior.handle(ev);
            }
        }
        if let Some((action, priority)) = self.behavior.take_pending() {
            if let Some(m) = model {
                let group = assets.action_group(action.name());
                let ok = m.start_motion(group, 0, priority);
                log_line(&format!(
                    "action: {:?} -> group={} priority={} ok={}",
                    action, group, priority, ok
                ));
            }
        }
    }

    unsafe fn init_model(
        gfx: &Gfx,
        assets: &CharacterAssets,
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
        log_line("step: locate model3");

        let model3 = assets.model3_path();
        let model3_str = model3.to_string_lossy().to_string();
        if !model3.exists() {
            log_line(&format!("model3 not found: {model3_str}"));
            return None;
        }

        log_line("step: load model");
        let model = CubismModel::load(&model3_str, width, height);
        if model.is_some() {
            log_line("live2d model ready");
        }
        model
    }
}
