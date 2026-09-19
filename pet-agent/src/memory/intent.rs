//! Memory Intent Detector：从用户文本识别「记住……」意图。
//!
//! 规则驱动（非 LLM），保守。
//! 显式记住 → 可自动写入 active（有日志）。
//! 其它值得记住的推测 → pending suggestion（不直接写 active）。

/// 检测显式记住意图，返回要记住的内容。
pub fn detect_explicit(text: &str) -> Option<String> {
    let t = text.trim();
    for prefix in [
        "记住：",
        "记住:",
        "记住 ",
        "请记住：",
        "请记住:",
        "帮我记住：",
        "帮我记住:",
    ] {
        if let Some(rest) = t.strip_prefix(prefix) {
            let c = rest.trim();
            if !c.is_empty() {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// 推测类型（保守，只用于生成 pending suggestion）。
pub fn suggest_kind(text: &str) -> &'static str {
    let t = text;
    if t.contains("喜欢") || t.contains("偏好") || t.contains("讨厌") {
        "preference"
    } else if t.contains("正在做") || t.contains("开发") || t.contains("项目") {
        "project"
    } else if t.contains("我叫") || t.contains("我的名字") || t.contains("朋友") {
        "relationship"
    } else {
        "fact"
    }
}

/// 是否像值得记住的声明（用于 implicit suggestion；保守）。
/// 疑问句不算（如「我喜欢喝什么？」）。
pub fn looks_memorable(text: &str) -> bool {
    let t = text.trim();
    if t.chars().count() > 60 {
        return false;
    }
    if t.ends_with('？') || t.ends_with('?') {
        return false;
    }
    let markers = [
        "我叫",
        "我的名字",
        "我喜欢",
        "我讨厌",
        "我正在",
        "我在做",
        "我的项目",
    ];
    markers.iter().any(|m| t.contains(m))
}

/// 检测显式修改意图（如「以后改成绿茶」「把红茶改成绿茶」）。
/// 返回要改成的新内容（尽力提取）。
pub fn detect_change(text: &str) -> Option<String> {
    let t = text.trim();
    for kw in ["改成", "改为", "换成", "换为"] {
        if let Some(pos) = t.find(kw) {
            // pos 为字节索引，kw.len() 为字节长度（中文关键字不可用 chars().count()）
            let after = t[pos + kw.len()..].trim();
            let after = after.trim_end_matches(['。', '.', '！', '!']).trim();
            if !after.is_empty() {
                return Some(after.to_string());
            }
        }
    }
    None
}
