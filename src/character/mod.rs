//! 角色抽象层。
//!
//! - package：解析 character.json、验证资产、语义动作映射
//! - manager：扫描 characters/、加载指定角色
//! - cubism：C++ shim 的 FFI 安全封装

pub mod cubism;
pub mod manager;
pub mod package;

pub use manager::CharacterManager;
pub use package::CharacterPackage;
