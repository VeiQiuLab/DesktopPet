//! Presentation / Expression Layer：桌宠的「表达」抽象。
//!
//! 边界：
//! - 只负责气泡显示、调度动作（经 Behavior）、生命周期。
//! - 动作仍走 Behavior → Scheduler → Character → Cubism，不绕过。
//!
//! 流程：
//!   PetExpression
//!     → PresentationController
//!         ├── Speech Bubble
//!         └── Motion / Behavior

pub mod controller;
pub mod expression;

pub use controller::PresentationController;
#[allow(unused_imports)]
pub use expression::{PetExpression, Priority};
