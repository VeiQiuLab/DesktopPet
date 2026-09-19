//! SpeechTextSanitizer：从助手文本中提取适合朗读的自然语言。
//!
//! 去除 Markdown / 代码块 / URL / 控制信息；压空白；限制长度。

/// 清理并截断为适合朗读的文本。
pub fn sanitize(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("~~~") || t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let t = t
            .trim_start_matches(|c| c == '#' || c == '>' || c == '-' || c == '*' || c == '+')
            .trim();
        if t.starts_with('{') || t.starts_with('[') || t.starts_with("motion:") {
            continue;
        }
        if !t.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(t);
        }
    }
    let cleaned = strip_urls(&out);
    let mut s = String::with_capacity(cleaned.len());
    let mut prev_space = false;
    for ch in cleaned.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                s.push(' ');
                prev_space = true;
            }
        } else {
            s.push(ch);
            prev_space = false;
        }
    }
    let s = s.trim().to_string();
    if s.chars().count() <= max_chars {
        s
    } else {
        let mut r: String = s.chars().take(max_chars).collect();
        r.push('…');
        r
    }
}

fn strip_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split(' ') {
        if token.starts_with("http://") || token.starts_with("https://") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token);
    }
    out
}
