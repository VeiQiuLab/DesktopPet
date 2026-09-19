//! Persona：桌宠「是谁」的稳定身份设定。
//!
//! 与 Live2D Character Package 解耦：Persona 属于 pet-agent，Character 属于 DesktopPet。
//! 通过 personas/<id>/persona.json 配置，不硬编码在 Provider/UI/Conversation。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    #[serde(default = "default_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// 核心 system prompt。
    pub system_prompt: String,
    #[serde(default)]
    pub style: Style,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    #[serde(default = "default_max_sentences")]
    pub max_sentences: u32,
    #[serde(default = "default_true")]
    pub prefer_concise: bool,
}

fn default_id() -> String {
    "default".into()
}
fn default_max_sentences() -> u32 {
    3
}
fn default_true() -> bool {
    true
}

impl Default for Style {
    fn default() -> Self {
        Style {
            max_sentences: default_max_sentences(),
            prefer_concise: true,
        }
    }
}

impl Default for Persona {
    fn default() -> Self {
        Persona {
            id: default_id(),
            name: "Default".into(),
            system_prompt: "你是桌面角色的对话层。回复尽量简短自然，通常 1～3 句话，适合显示在小小的桌面气泡里。不要输出 Markdown 或大段格式。".into(),
            style: Style::default(),
        }
    }
}

impl Persona {
    /// 从 personas/<id>/persona.json 加载；缺失则回退内置默认并写出模板。
    pub fn load(id: &str) -> Self {
        let path = persona_path(id);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Persona>(&text) {
                Ok(p) => p,
                Err(e) => {
                    crate::log_line(&format!("persona parse error ({e}), using default"));
                    Persona::default()
                }
            },
            Err(_) => {
                // 写出模板便于用户编辑
                let p = if id == "default" {
                    Persona::default()
                } else {
                    let mut d = Persona::default();
                    d.id = id.to_string();
                    d.name = id.to_string();
                    d
                };
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Ok(t) = serde_json::to_string_pretty(&p) {
                    let _ = std::fs::write(&path, t);
                }
                p
            }
        }
    }

    /// 当前激活的 persona id（可由配置指定；此处固定 default）。
    pub fn active_id() -> String {
        "default".into()
    }
}

fn persona_path(id: &str) -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    // exe 同目录优先，其次项目根（开发回退）
    let exe_personas = p.join("personas");
    let base = if exe_personas.is_dir() {
        exe_personas
    } else {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("personas");
        manifest
    };
    base.join(id).join("persona.json")
}
