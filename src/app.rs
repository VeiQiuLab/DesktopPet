//! 应用生命周期与主循环。
//!
//! 流程：Config → CharacterManager → CharacterPackage → Cubism model。
//! 每帧：泵消息 → 消费事件 → Behavior Scheduler → 动作 → 渲染。
//! 隐藏时暂停渲染，仅泵消息，显著降低资源占用。
//! 视线跟随：每帧 GetCursorPos 查询，不使用队列。

use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::behavior::{BehaviorController, PetAction, PetEvent, TickContext};
use crate::character::cubism::CubismModel;
use crate::character::{CharacterManager, CharacterPackage};
use crate::config::{log_line, Config};
use crate::ipc::protocol::{ValidatedCommand, ValidatedRequest};
use crate::ipc::server::IpcServer;
use crate::platform::gfx::Gfx;
use crate::platform::window::{
    create_window, is_dragging, is_menu_open, set_active_character, set_model, WindowParams,
};
use crate::presentation::{PetExpression, PresentationController};

const LOOK_RADIUS: f32 = 1.5;
const FRAME_MS: u64 = 16;
const HIDDEN_FRAME_MS: u64 = 100;

pub struct App {
    config: Config,
    events_rx: Option<Receiver<PetEvent>>,
}

impl App {
    pub fn new() -> Self {
        App {
            config: Config::load(),
            events_rx: None,
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

        // 扫描角色包（仅启动一次）
        let characters_root = Self::characters_root();
        let manager = CharacterManager::scan(&characters_root);

        let char_list: Vec<(String, String)> = manager
            .list()
            .iter()
            .map(|p| (p.id().to_string(), p.display_name().to_string()))
            .collect();

        let resolved_id = manager
            .resolve_active(&self.config.character.active_character)
            .map(|p| p.id().to_string())
            .unwrap_or_default();
        if !resolved_id.is_empty() {
            self.config.character.active_character = resolved_id.clone();
        }

        unsafe {
            let (tx, rx) = channel::<PetEvent>();
            self.events_rx = Some(rx);

            let hwnd: HWND = create_window(
                &params,
                self.config.clone(),
                tx,
                char_list,
                resolved_id.clone(),
            )?;
            let gfx = Gfx::new(hwnd, win_w, win_h)?;

            let mut model: Option<CubismModel> = None;
            let mut behavior: Option<BehaviorController> = None;
            let mut presentation: Option<PresentationController> = None;

            if let Some(pkg) = manager.resolve_active(&resolved_id) {
                let idle = pkg.idle_behavior().clone();
                model = Self::init_model(&gfx, pkg, win_w, win_h);
                behavior = Some(BehaviorController::new(idle));
                presentation = Some(PresentationController::new(pkg));
            } else {
                log_line("no character available");
            }

            if let Some(m) = &model {
                set_model(hwnd, m.raw_handle());
            }
            if let Some(p) = presentation.as_mut() {
                p.greet();
            }

            // 启动 IPC server（Named Pipe）
            let (ipc_server, ipc_rx, ipc_status) = IpcServer::start();
            log_line("ipc server started");

            let mut last = Instant::now();
            let mut msg = MSG::default();
            let mut quit = false;

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

                let visible = IsWindowVisible(hwnd).as_bool();
                // 更新供 IPC 只读查询的状态快照
                {
                    let mut rc = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rc);
                    ipc_status.set_visible(visible);
                    ipc_status.set_rect(rc.left, rc.top, rc.right, rc.bottom);
                    ipc_status.set_active_character(&self.config.character.active_character);
                }

                // 处理事件（角色切换 / 行为 / 气泡反馈）
                self.process_events(
                    hwnd,
                    &gfx,
                    &manager,
                    &mut model,
                    &mut behavior,
                    &mut presentation,
                    win_w,
                    win_h,
                    &mut quit,
                );
                if quit {
                    break 'running;
                }

                // 处理 IPC 请求
                self.process_ipc(
                    hwnd,
                    &gfx,
                    &manager,
                    &mut model,
                    &mut behavior,
                    &mut presentation,
                    &ipc_rx,
                    win_w,
                    win_h,
                    &mut quit,
                );
                if quit {
                    break 'running;
                }

                let blocked = is_dragging(hwnd) || is_menu_open(hwnd);

                if visible {
                    if let (Some(b), Some(m)) = (behavior.as_mut(), model.as_ref()) {
                        let ctx = TickContext {
                            is_motion_busy: m.is_busy(),
                            interaction_blocked: blocked,
                        };
                        if let Some(action) = b.tick(dt, &ctx) {
                            match action {
                                PetAction::Quit => {
                                    quit = true;
                                }
                                PetAction::ResetPosition => Self::reset_position(),
                                _ => {
                                    let active_id = self.config.character.active_character.clone();
                                    Self::apply_motion(m, action, &manager, &active_id);
                                }
                            }
                        }
                    }

                    // Presentation：气泡生命周期 + idle speech；返回的动作交给 Behavior 执行
                    if let Some(p) = presentation.as_mut() {
                        if let Some(action) = p.tick_at(dt, visible, blocked, hwnd) {
                            if let Some(m) = model.as_ref() {
                                let active_id = self.config.character.active_character.clone();
                                Self::apply_motion(m, action, &manager, &active_id);
                            }
                        }
                        p.reposition(hwnd);
                    }

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
                    let _ = gfx.present();

                    Self::sleep_remaining(now, FRAME_MS);
                } else {
                    // 隐藏：暂停 idle speech，隐藏气泡
                    if let Some(p) = presentation.as_mut() {
                        let _ = p.tick(0.0, false, true);
                    }
                    Self::sleep_remaining(now, HIDDEN_FRAME_MS);
                }
            }

            // 停止 IPC（Drop 会 join worker）
            drop(ipc_server);

            // 统一退出流程：销毁窗口 → 触发 WM_DESTROY（保存配置、移除托盘）
            let _ = DestroyWindow(hwnd);

            if model.is_some() {
                CubismModel::shutdown();
            }
            log_line("main loop exited");
        }
        Ok(())
    }

    fn sleep_remaining(start: Instant, target_ms: u64) {
        let elapsed = start.elapsed();
        let target = Duration::from_millis(target_ms);
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
    }

    fn characters_root() -> std::path::PathBuf {
        let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
        p.pop();
        let exe_chars = p.join("characters");
        if exe_chars.is_dir() {
            return exe_chars;
        }
        let manifest_chars =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("characters");
        if manifest_chars.is_dir() {
            return manifest_chars;
        }
        exe_chars
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn process_events(
        &mut self,
        hwnd: HWND,
        gfx: &Gfx,
        manager: &CharacterManager,
        model: &mut Option<CubismModel>,
        behavior: &mut Option<BehaviorController>,
        presentation: &mut Option<PresentationController>,
        win_w: i32,
        win_h: i32,
        quit: &mut bool,
    ) {
        // 先收集（结束对 self.events_rx 的借用）
        let events: Vec<PetEvent> = match &self.events_rx {
            Some(rx) => rx.try_iter().collect(),
            None => Vec::new(),
        };

        let mut switch_to: Option<String> = None;
        let mut test_bubble = false;
        for ev in events {
            match ev {
                PetEvent::TraySwitchCharacter(id) => switch_to = Some(id),
                PetEvent::MenuTestBubble => test_bubble = true,
                PetEvent::LeftClick => {
                    if let Some(p) = presentation.as_mut() {
                        p.on_click();
                    }
                    if let Some(b) = behavior.as_mut() {
                        b.handle(PetEvent::LeftClick);
                    }
                }
                PetEvent::DoubleClick => {
                    if let Some(p) = presentation.as_mut() {
                        p.on_double_click();
                    }
                    if let Some(b) = behavior.as_mut() {
                        b.handle(PetEvent::DoubleClick);
                    }
                }
                PetEvent::DragStart => {
                    if let Some(p) = presentation.as_mut() {
                        p.on_interaction();
                    }
                    if let Some(b) = behavior.as_mut() {
                        b.handle(PetEvent::DragStart);
                    }
                }
                other => {
                    if let Some(b) = behavior.as_mut() {
                        b.handle(other);
                    }
                }
            }
        }

        if let Some(new_id) = switch_to {
            Self::switch_character(
                hwnd,
                gfx,
                manager,
                model,
                behavior,
                presentation,
                win_w,
                win_h,
                &new_id,
                &mut self.config,
            );
        }

        // 托盘「测试气泡」：走 PetExpression → Presentation（不直接创建气泡）
        if test_bubble {
            if let Some(p) = presentation.as_mut() {
                if let Some(action) = p.present(PetExpression::TextAndMotion {
                    text: "桌宠运行正常。".to_string(),
                    action: crate::behavior::PetAction::Nod,
                    priority: crate::presentation::Priority::System,
                    duration: None,
                }) {
                    if let Some(m) = model.as_ref() {
                        let active_id = self.config.character.active_character.clone();
                        Self::apply_motion(m, action, manager, &active_id);
                    }
                }
            }
        }
        let _ = quit;
    }

    /// 处理 IPC 请求（worker 线程校验后经有界 channel 送达）。
    #[allow(clippy::too_many_arguments)]
    unsafe fn process_ipc(
        &mut self,
        hwnd: HWND,
        gfx: &Gfx,
        manager: &CharacterManager,
        model: &mut Option<CubismModel>,
        behavior: &mut Option<BehaviorController>,
        presentation: &mut Option<PresentationController>,
        ipc_rx: &Receiver<ValidatedRequest>,
        win_w: i32,
        win_h: i32,
        quit: &mut bool,
    ) {
        const MAX_PER_FRAME: usize = 8;
        for _ in 0..MAX_PER_FRAME {
            let req = match ipc_rx.try_recv() {
                Ok(r) => r,
                Err(_) => break,
            };
            match req {
                ValidatedRequest::Expression(e) => {
                    let priority = crate::ipc::protocol::priority_to_presentation(e.priority);
                    let duration_s = e.duration_ms.map(|ms| ms as f32 / 1000.0);
                    let expr = match (e.text, e.motion) {
                        (Some(text), Some(m)) => PetExpression::TextAndMotion {
                            text,
                            action: crate::ipc::protocol::motion_to_action(m),
                            priority,
                            duration: duration_s,
                        },
                        (Some(text), None) => PetExpression::Text {
                            text,
                            priority,
                            duration: duration_s,
                        },
                        (None, Some(m)) => PetExpression::Motion {
                            action: crate::ipc::protocol::motion_to_action(m),
                            priority,
                        },
                        (None, None) => continue,
                    };
                    if let Some(p) = presentation.as_mut() {
                        if let Some(action) = p.present(expr) {
                            if let Some(m) = model.as_ref() {
                                let active_id = self.config.character.active_character.clone();
                                Self::apply_motion(m, action, manager, &active_id);
                            }
                        }
                    }
                }
                ValidatedRequest::Query { .. } => {
                    // query 已在 IPC worker 内直接应答，这里不会到达
                }
                ValidatedRequest::Command { command, .. } => match command {
                    ValidatedCommand::Show => {
                        crate::platform::window::show_window(hwnd);
                    }
                    ValidatedCommand::Hide => {
                        crate::platform::window::hide_window(hwnd);
                    }
                    ValidatedCommand::ResetPosition => Self::reset_position(),
                    ValidatedCommand::DismissBubble => {
                        if let Some(p) = presentation.as_mut() {
                            p.hide();
                        }
                    }
                    ValidatedCommand::Character(id) => {
                        Self::switch_character(
                            hwnd,
                            gfx,
                            manager,
                            model,
                            behavior,
                            presentation,
                            win_w,
                            win_h,
                            &id,
                            &mut self.config,
                        );
                    }
                },
            }
        }
        let _ = quit;
    }

    /// 供未来 AI / 外部源提交表达。当前未接线到主循环输入，仅作为公开接口预留。
    #[allow(dead_code)]
    pub fn submit_expression(presentation: &mut Option<PresentationController>, text: &str) {
        if let Some(p) = presentation.as_mut() {
            let _ = p.present(PetExpression::user_text(text));
        }
    }

    fn apply_motion(
        m: &CubismModel,
        action: PetAction,
        manager: &CharacterManager,
        active_id: &str,
    ) {
        if let Some(pkg) = manager.get(active_id) {
            if let Some(group) = pkg.action_group(action.name()) {
                let ok = m.start_motion(&group, 0, Self::priority_of(action));
                crate::config::log_debug(&format!("action: {action:?} -> group={group} ok={ok}"));
            }
        }
    }

    fn priority_of(action: PetAction) -> i32 {
        match action {
            // Blink/Nod 为普通动作（可抢占 Idle 优先级 1），Shake 为强制动作。
            PetAction::Blink => 2,
            PetAction::Nod => 2,
            PetAction::Shake => 3,
            _ => 1,
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn switch_character(
        hwnd: HWND,
        gfx: &Gfx,
        manager: &CharacterManager,
        model: &mut Option<CubismModel>,
        behavior: &mut Option<BehaviorController>,
        presentation: &mut Option<PresentationController>,
        win_w: i32,
        win_h: i32,
        new_id: &str,
        config: &mut Config,
    ) {
        let pkg: &CharacterPackage = match manager.get(new_id) {
            Some(p) => p,
            None => {
                log_line(&format!("switch failed: character '{new_id}' not found"));
                return;
            }
        };

        // 卸载旧模型（Drop 会释放句柄），并清除命中测试句柄
        *model = None;
        set_model(hwnd, std::ptr::null_mut());

        let new_model = Self::init_model(gfx, pkg, win_w, win_h);
        match new_model {
            Some(m) => {
                set_model(hwnd, m.raw_handle());
                let idle = pkg.idle_behavior().clone();
                match behavior.as_mut() {
                    Some(b) => b.set_idle(idle),
                    None => *behavior = Some(BehaviorController::new(idle)),
                }
                // 更新 Presentation 的台词配置并清空旧气泡
                match presentation.as_mut() {
                    Some(p) => p.set_character(pkg),
                    None => *presentation = Some(PresentationController::new(pkg)),
                }
                *model = Some(m);
                config.character.active_character = new_id.to_string();
                config.save();
                set_active_character(hwnd, new_id);
                log_line(&format!("switched to character: {new_id}"));
            }
            None => {
                log_line(&format!("switch failed: cannot load model for '{new_id}'"));
                // 尝试恢复原角色
                let old = config.character.active_character.clone();
                if old != new_id {
                    if let Some(old_pkg) = manager.get(&old) {
                        if let Some(m) = Self::init_model(gfx, old_pkg, win_w, win_h) {
                            set_model(hwnd, m.raw_handle());
                            *model = Some(m);
                            log_line(&format!("restored previous character: {old}"));
                        }
                    }
                }
            }
        }
    }

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
            crate::config::log_debug(&format!("reset position to ({nx},{ny})"));
        }
    }

    fn compute_look(hwnd: HWND, win_w: i32, win_h: i32) -> Option<(f32, f32)> {
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let rx = pt.x - rect.left;
            let ry = pt.y - rect.top;
            let max_x = (win_w as f32) * LOOK_RADIUS;
            let max_y = (win_h as f32) * LOOK_RADIUS;
            if (rx as f32) < -max_x
                || (rx as f32) > max_x
                || (ry as f32) < -max_y
                || (ry as f32) > max_y
            {
                return None;
            }
            let nx = (rx as f32 / win_w as f32) * 2.0 - 1.0;
            let ny = 1.0 - (ry as f32 / win_h as f32) * 2.0;
            Some((nx.clamp(-1.0, 1.0), ny.clamp(-1.0, 1.0)))
        }
    }

    unsafe fn init_model(
        gfx: &Gfx,
        pkg: &CharacterPackage,
        width: i32,
        height: i32,
    ) -> Option<CubismModel> {
        if !CubismModel::startup() {
            log_line("cubism startup failed");
            return None;
        }
        CubismModel::set_device(gfx.device_ptr(), gfx.context_ptr());
        let model3 = pkg.model3.to_string_lossy().to_string();
        CubismModel::load(&model3, width, height, pkg.scale, pkg.offset())
    }
}
