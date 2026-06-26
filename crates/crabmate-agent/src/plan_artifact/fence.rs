/// 围栏内首段非空行若为 `json` / `markdown` / `md`（忽略大小写），返回剥去该行及前导空行后的正文（已 trim）；否则 `None`。
///
/// 与流式缓冲判定共用：无语言行时**不得**把「正文以 `{` 开头」当作规划 JSON（避免裸 \`\`\` 思维链误判）。
pub fn fenced_body_after_optional_jsonish_lang_label(raw: &str) -> Option<String> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0usize;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    if i >= lines.len() {
        return None;
    }
    let first_t = lines[i].trim();
    if first_t.eq_ignore_ascii_case("json")
        || first_t.eq_ignore_ascii_case("markdown")
        || first_t.eq_ignore_ascii_case("md")
    {
        Some(lines[i + 1..].join("\n").trim().to_string())
    } else {
        None
    }
}

/// 围栏内首段非空行若为 `json` / `markdown` / `md`（忽略大小写），则剥去该行及之前的前导空行；否则返回 `raw.trim()`。
pub fn strip_optional_json_fence_label(raw: &str) -> String {
    fenced_body_after_optional_jsonish_lang_label(raw).unwrap_or_else(|| raw.trim().to_string())
}
