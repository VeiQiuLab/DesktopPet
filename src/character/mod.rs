//! 角色抽象层。
//!
//! 第一阶段仅定义接口占位：角色资源路径与加载入口。
//! 里程碑2 已由 C++ Cubism shim 实现实际渲染。
//! 未来可并列 Sprite / Live2D 等多种 renderer 实现。

pub mod cubism;

use std::path::PathBuf;

use crate::config::CharacterConfig;

/// 已解析的角色资产信息（里程碑2 接入实际渲染时启用）。
#[allow(dead_code)]
pub struct CharacterAssets {
    /// 资产目录绝对路径
    pub dir: PathBuf,
    /// 模型名（目录内 <name>.model3.json）
    pub name: String,
}

#[allow(dead_code)]
impl CharacterAssets {
    /// 从配置解析资产路径。
    pub fn from_config(cfg: &CharacterConfig) -> Self {
        CharacterAssets {
            dir: PathBuf::from(&cfg.asset_dir),
            name: cfg.model_name.clone(),
        }
    }

    /// model3.json 的完整路径。
    pub fn model3_path(&self) -> PathBuf {
        self.dir.join(format!("{}.model3.json", self.name))
    }

    /// 语义动作 → 实际 motion group 名。
    ///
    /// 第一版直接映射到 model3.json 中的 group；未来换角色时可通过配置/资产重新定义。
    pub fn action_group(&self, action_name: &str) -> &'static str {
        match action_name {
            "Idle" => "Idle",
            "Blink" => "Blink",
            "Nod" => "Nod",
            "Shake" => "Shake",
            _ => "Idle",
        }
    }
}
