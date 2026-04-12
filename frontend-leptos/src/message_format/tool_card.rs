//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolResultInfo` 对齐）。

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;

use super::plain::collapse_duplicate_summary_lines;

pub fn tool_card_text(info: &ToolResultInfo, loc: Locale) -> String {
    let sum = info.summary.as_deref().unwrap_or("").trim();
    let name = info.name.trim();
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    let sum = collapse_duplicate_summary_lines(sum);
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    // 首行 + 其余行；其余行中再剔除与首行相同的行，避免「标题行 + 正文重复首行」。
    let mut lines = sum.lines();
    let first = lines.next().unwrap_or_default().trim().to_string();
    if first.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    let rest: Vec<&str> = lines
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != first.as_str())
        .collect();
    if rest.is_empty() {
        return first;
    }
    let mut out = first;
    out.push_str("\n\n");
    out.push_str(&rest.join("\n"));
    out
}
