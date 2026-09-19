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
    /// 角色专属台词（可选）。
    #[serde(default)]
    pub speech: SpeechMeta,
    /// 语义参数映射（可选）。
    #[serde(default)]
    pub parameters: ParametersMeta,
}

/// 语义参数 → Cubism 参数 ID 映射。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametersMeta {
    /// 嘴型开合参数 ID。缺省使用 "ParamMouthOpenY"。
    #[serde(default = "default_mouth_open")]
    pub mouth_open: String,
}

fn default_mouth_open() -> String {
    "ParamMouthOpenY".to_string()
}

impl Default for ParametersMeta {
    fn default() -> Self {
        ParametersMeta {
            mouth_open: default_mouth_open(),
        }
    }
}

/// 角色专属台词配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpeechMeta {
    /// 启动问候语（可选）。
    #[serde(default)]
    pub greeting: Option<String>,
    /// 空闲台词池。
    #[serde(default)]
    pub idle: Vec<String>,
    /// 单击台词池。
    #[serde(default)]
    pub click: Vec<String>,
    /// 双击台词池。
    #[serde(default)]
    pub double_click: Vec<String>,
    /// 单击时说话的频率（0..1）。
    #[serde(default = "default_click_speech_prob")]
    pub click_speech_probability: f32,
    /// 双击时说话的频率（0..1）。
    #[serde(default = "default_double_click_speech_prob")]
    pub double_click_speech_probability: f32,
    /// 空闲台词间隔下限（秒）。
    #[serde(default = "default_idle_speech_min")]
    pub idle_speech_interval_min: f32,
    /// 空闲台词间隔上限（秒）。
    #[serde(default = "default_idle_speech_max")]
    pub idle_speech_interval_max: f32,
    /// 用户交互后抑制空闲台词的时长（秒）。
    #[serde(default = "default_idle_speech_suppress")]
    pub idle_speech_suppress_after_interaction: f32,
}

fn default_click_speech_prob() -> f32 {
    0.4
}
fn default_double_click_speech_prob() -> f32 {
    0.6
}
fn default_idle_speech_min() -> f32 {
    60.0
}
fn default_idle_speech_max() -> f32 {
    180.0
}
fn default_idle_speech_suppress() -> f32 {
    20.0
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
    #[serde(default)]
    pub idle: IdleBehavior,
}

impl Default for BehaviorMeta {
    fn default() -> Self {
        BehaviorMeta {
            double_click_ms: default_double_click_ms(),
            drag_threshold_px: default_drag_threshold(),
            idle: IdleBehavior::default(),
        }
    }
}

/// 空闲随机行为参数（秒 / 概率）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleBehavior {
    /// Blink 间隔下限（秒）。
    #[serde(default = "default_blink_min")]
    pub blink_interval_min: f32,
    /// Blink 间隔上限（秒）。
    #[serde(default = "default_blink_max")]
    pub blink_interval_max: f32,
    /// 每次空闲 tick 触发 Nod 的概率。
    #[serde(default = "default_nod_prob")]
    pub nod_probability: f32,
    /// 每次空闲 tick 触发 Shake 的概率。
    #[serde(default = "default_shake_prob")]
    pub shake_probability: f32,
    /// 动作结束后冷却（秒），冷却期内不触发随机动作。
    #[serde(default = "default_action_cooldown")]
    pub action_cooldown: f32,
    /// 两次随机动作之间的最小间隔（秒）。
    #[serde(default = "default_min_interval")]
    pub min_interval: f32,
}

impl Default for IdleBehavior {
    fn default() -> Self {
        IdleBehavior {
            blink_interval_min: default_blink_min(),
            blink_interval_max: default_blink_max(),
            nod_probability: default_nod_prob(),
            shake_probability: default_shake_prob(),
            action_cooldown: default_action_cooldown(),
            min_interval: default_min_interval(),
        }
    }
}

fn default_blink_min() -> f32 {
    3.0
}
fn default_blink_max() -> f32 {
    7.0
}
fn default_nod_prob() -> f32 {
    0.15
}
fn default_shake_prob() -> f32 {
    0.03
}
fn default_action_cooldown() -> f32 {
    2.0
}
fn default_min_interval() -> f32 {
    2.0
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

        // model3 路径存在（禁止绝对路径与 .. 越出角色根）
        let model3_rel = PathBuf::from(&meta.model.model3);
        if model3_rel.is_absolute() {
            return Err(format!(
                "character.json: model3 must be relative, got {}",
                meta.model.model3
            ));
        }
        if model3_rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!(
                "character.json: model3 must not contain '..' (path escape): {}",
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

    pub fn idle_behavior(&self) -> &IdleBehavior {
        &self.meta.behavior.idle
    }

    pub fn speech(&self) -> &SpeechMeta {
        &self.meta.speech
    }
}
