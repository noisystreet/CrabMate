//! 时间线/气泡展示用纯文本拼装（无 `ChatStreamCallbackCtx` 副作用）。

pub(crate) fn non_empty_trimmed_tool_name(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub(crate) fn build_final_response_text(title: &str, detail: Option<&str>) -> String {
    let mut final_text = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !final_text.is_empty() {
            final_text.push_str("\n\n");
        }
        final_text.push_str(detail);
    }
    final_text
}

pub(crate) fn to_single_line(s: &str, max_chars: usize) -> String {
    let compact = s
        .split_whitespace()
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}
