//! 纯文本行级整理（与协议/角色无关）。

/// 去掉摘要里**连续重复**的非空行（服务端或上游偶发会下发两行相同摘要，如 `read file: 2.md`）。
pub fn collapse_duplicate_summary_lines(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut last: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if last == Some(t) {
            continue;
        }
        last = Some(t);
        kept.push(t);
    }
    kept.join("\n")
}

/// 将连续的空行（仅含空白字符的行）压缩为至多一行空段，减轻剥 tag / 围栏后产生的 `\n\n\n+`。
pub fn collapse_consecutive_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut in_blank_run = true;
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank {
            if !in_blank_run && !out.is_empty() {
                out.push('\n');
            }
            in_blank_run = true;
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
            in_blank_run = false;
        }
    }
    out
}
