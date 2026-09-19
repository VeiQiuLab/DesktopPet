//! Presentation / Expression Layer：桌宠向用户的「表达」抽象。
//!
//! 边界：
//! - 未来 AI / 外部源只提交 PetExpression，不接触 HWND / Cubism / D3D11。
//! - PresentationController 负责：气泡显示、调度动作（经 Behavior）、生命周期。
//! - 动作仍走 Behavior → Scheduler → Character → Cubism，不绕过。
//!
//! 流程：
//!   External Source / Future AI
//!     → PetExpression
//!     → PresentationController
//!         ├── Speech Bubble
//!         ├── Motion / Behavior
//!         └── Future Facial Expression

pub mod controller;
pub mod expression;

pub use controller::PresentationController;
#[allow(unused_imports)]
pub use expression::{PetExpression, Priority};
