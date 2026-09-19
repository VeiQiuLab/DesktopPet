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

/// 扫描到的 persona 摘要。
#[derive(Debug, Clone)]
pub struct PersonaInfo {
    pub id: String,
    pub name: String,
}

/// personas 根目录（exe 同目录优先，其次项目根）。
fn personas_root() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    p.pop();
    let exe_personas = p.join("personas");
    if exe_personas.is_dir() {
        return exe_personas;
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("personas")
}

/// 扫描并校验全部 persona（坏包跳过 + warning）。
pub fn scan() -> Vec<PersonaInfo> {
    let root = personas_root();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let id = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            match validate(&dir) {
                Ok(p) => out.push(PersonaInfo { id, name: p.name }),
                Err(w) => crate::log_line(&format!("persona '{id}' skipped: {w}")),
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// 校验 persona 目录。
fn validate(dir: &std::path::Path) -> Result<Persona, String> {
    let path = dir.join("persona.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read failed: {e}"))?;
    let p: Persona = serde_json::from_str(&text).map_err(|e| format!("parse failed: {e}"))?;
    if p.id.trim().is_empty() {
        return Err("id empty".into());
    }
    if p.name.trim().is_empty() {
        return Err("name empty".into());
    }
    if p.system_prompt.trim().is_empty() {
        return Err("system_prompt empty".into());
    }
    if !(1..=20).contains(&p.style.max_sentences) {
        return Err("style.max_sentences out of range".into());
    }
    Ok(p)
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

    /// 加载指定 id，找不到回退 default（+ warning）。
    pub fn load_or_default(id: &str) -> Self {
        let p = Self::load(id);
        if p.id != id && !id.is_empty() {
            crate::log_line(&format!("persona '{id}' not found, using default"));
            return Self::load("default");
        }
        p
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
