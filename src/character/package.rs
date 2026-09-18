//! 角色包（Character Package）：元数据 + 解析后的资产路径。
//!
//! 目录约定：
//! characters/
//! └── <id>/
//!     ├── character.json
//!     └── model/            # 可选，默认可放任意位置，由 character.json 的 model3 指定
//!         ├── *.model3.json
//!         ├── *.moc3
//!         ├── *.physics3.json
//!         ├── *.cdi3.json
//!         ├── *.motion3.json
//!         └── texture...
//!
//! Runtime 只认识语义动作：Idle / Blink / Nod / Shake；
//! 具体映射由 character.json 的 motions 字段决定。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// character.json 顶层结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterMeta {
    /// 唯一 id（应与目录名一致）。
    pub id: String,
    /// 显示名（UI 用）。
    #[serde(default)]
    pub display_name: String,
    /// 模型信息。
    pub model: ModelMeta,
    /// 语义动作 → Cubism motion group 名。缺省时使用同名 group。
    #[serde(default)]
    pub motions: BTreeMap<String, String>,
    /// 行为参数。
    #[serde(default)]
    pub behavior: BehaviorMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMeta {
    /// 相对角色根目录的 model3.json 路径。
    pub model3: String,
    /// 默认缩放（1.0 = 原始大小）。范围 [0.1, 5.0]，越界会被 clamp。
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// X 方向偏移（canvas 单位）。
    #[serde(default)]
    pub offset_x: f32,
    /// Y 方向偏移（canvas 单位）。
    #[serde(default)]
    pub offset_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorMeta {
    #[serde(default = "default_double_click_ms")]
    pub double_click_ms: u64,
    #[serde(default = "default_drag_threshold")]
    pub drag_threshold_px: i32,
}

impl Default for BehaviorMeta {
    fn default() -> Self {
        BehaviorMeta {
            double_click_ms: default_double_click_ms(),
            drag_threshold_px: default_drag_threshold(),
        }
    }
}

fn default_scale() -> f32 {
    1.0
}
fn default_double_click_ms() -> u64 {
    400
}
fn default_drag_threshold() -> i32 {
    5
}

/// 已解析 + 验证通过的角色包。
#[derive(Debug, Clone)]
pub struct CharacterPackage {
    /// 角色根目录（绝对）。
    #[allow(dead_code)]
    pub root: PathBuf,
    /// 元数据。
    pub meta: CharacterMeta,
    /// model3.json 的绝对路径。
    pub model3: PathBuf,
    /// 实际使用的 scale（已 clamp）。
    pub scale: f32,
}

impl CharacterPackage {
    /// 从角色根目录加载并验证。
    ///
    /// 错误时返回 Err(原因)，但不会 panic 或产生 access violation。
    pub fn load(root: &Path) -> Result<Self, String> {
        if !root.is_dir() {
            return Err(format!("not a directory: {}", root.display()));
        }

        let meta_path = root.join("character.json");
        let text = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("read {} failed: {e}", meta_path.display()))?;
        let meta: CharacterMeta = serde_json::from_str(&text)
            .map_err(|e| format!("parse {} failed: {e}", meta_path.display()))?;

        // id 非空
        if meta.id.trim().is_empty() {
            return Err("character.json: id is empty".to_string());
        }

        // model3 路径存在
        let model3_rel = PathBuf::from(&meta.model.model3);
        if model3_rel.is_absolute() {
            return Err(format!(
                "character.json: model3 must be relative, got {}",
                meta.model.model3
            ));
        }
        let model3 = root.join(&model3_rel);
        if !model3.is_file() {
            return Err(format!("model3 not found: {}", model3.display()));
        }

        // scale clamp
        let scale = if meta.model.scale.is_finite() {
            meta.model.scale.clamp(0.1, 5.0)
        } else {
            1.0
        };

        Ok(CharacterPackage {
            root: root.to_path_buf(),
            meta,
            model3,
            scale,
        })
    }

    /// 语义动作 → Cubism motion group。返回 None 表示该角色无此动作。
    pub fn action_group(&self, semantic: &str) -> Option<String> {
        if let Some(g) = self.meta.motions.get(semantic) {
            return Some(g.clone());
        }
        // 缺省：与语义同名（若 model3.json 里存在同名 group）
        Some(semantic.to_string())
    }

    pub fn id(&self) -> &str {
        &self.meta.id
    }

    pub fn display_name(&self) -> &str {
        if self.meta.display_name.is_empty() {
            &self.meta.id
        } else {
            &self.meta.display_name
        }
    }

    pub fn offset(&self) -> (f32, f32) {
        (self.meta.model.offset_x, self.meta.model.offset_y)
    }
}
