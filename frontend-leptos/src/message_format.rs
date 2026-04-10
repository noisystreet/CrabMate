//! 消息与工具摘要的展示用字符串处理（含 `agent_reply_plan` 围栏与流式缓冲语义）。

use std::borrow::Cow;

use serde_json::Value;

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;
use crate::storage::StoredMessage;

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

fn format_agent_reply_plan_json_for_display(
    json_text: &str,
    goal: &str,
    loc: Locale,
) -> Option<String> {
    let v: Value = serde_json::from_str(json_text).ok()?;
    let obj = v.as_object()?;
    if obj.get("type").and_then(|x| x.as_str()) != Some("agent_reply_plan") {
        return None;
    }
    let steps = obj.get("steps").and_then(|x| x.as_array())?;

    let mut lines = Vec::with_capacity(steps.len().saturating_add(1));
    let goal = goal.trim();
    if !goal.is_empty() {
        lines.push(goal.to_string());
    }
    if steps.is_empty() {
        if !goal.is_empty() {
            return Some(goal.to_string());
        }
        return Some(crate::i18n::plan_generated(loc).to_string());
    }
    if !goal.is_empty() {
        lines.push(String::new());
    }
    for (idx, s) in steps.iter().enumerate() {
        let id = s
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or(crate::i18n::plan_step_placeholder_id());
        let desc = s
            .get("description")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or(crate::i18n::plan_step_no_desc(loc));
        lines.push(crate::i18n::plan_step_line(
            loc,
            idx,
            id.trim(),
            desc.trim(),
        ));
    }
    Some(lines.join("\n"))
}

fn fenced_body_after_optional_jsonish_lang_label(inner: &str) -> Option<&str> {
    let s = inner.trim_start_matches(['\n', '\r', ' ', '\t']);
    if s.is_empty() {
        return Some("");
    }
    for label in ["json", "markdown", "md"] {
        if let Some(rest) = s.strip_prefix(label) {
            let mut chars = rest.chars();
            let next = chars.next();
            // 兼容两种形态：
            // 1) ```json\n{...}
            // 2) ```json{...}
            if next.is_none()
                || next == Some('\n')
                || next == Some('\r')
                || next == Some(' ')
                || next == Some('\t')
                || next == Some('{')
                || next == Some('[')
            {
                return Some(rest.trim_start_matches(['\n', '\r', ' ', '\t']));
            }
        }
    }
    None
}

fn triple_backtick_fence_count(s: &str) -> usize {
    s.match_indices("```").count()
}

fn first_fence_inner_looks_like_json_object(s: &str) -> bool {
    let mut it = s.split("```");
    let _ = it.next();
    let Some(inner) = it.next() else {
        return false;
    };
    let Some(body) = fenced_body_after_optional_jsonish_lang_label(inner) else {
        return false;
    };
    let b = body.trim();
    b.is_empty() || b.starts_with('{')
}

fn looks_like_incomplete_agent_reply_plan_whole_json(t: &str) -> bool {
    let t = t.trim();
    if !t.starts_with('{') {
        return false;
    }
    if t.contains("\"agent_reply_plan\"") {
        return true;
    }
    t.contains("\"type\"") && t.contains("\"version\"") && t.contains("\"steps\"")
}

fn should_buffer_agent_reply_plan_stream(stripped: &str) -> bool {
    if triple_backtick_fence_count(stripped) % 2 == 1
        && first_fence_inner_looks_like_json_object(stripped)
    {
        return true;
    }
    let t = stripped.trim();
    if !t.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(t, "", Locale::ZhHans).is_some() {
        return false;
    }
    serde_json::from_str::<Value>(t).is_err()
        && looks_like_incomplete_agent_reply_plan_whole_json(t)
}

fn prose_before_first_fence(s: &str) -> String {
    s.split("```").next().unwrap_or("").trim().to_string()
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = fenced_body_after_optional_jsonish_lang_label(raw)
        .unwrap_or(raw)
        .trim();
    if !body.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(body, "", Locale::ZhHans).is_some() {
        return true;
    }
    if !body.contains("\"agent_reply_plan\"") || !body.contains("\"steps\"") {
        return false;
    }
    serde_json::from_str::<Value>(body).is_ok()
}

/// 首段围栏前文本是否可视为「仅规划说明」从而省略，避免误删「大段正文 + 文末 plan 围栏」。
fn drop_first_segment_before_hidden_agent_reply_plan_fence(segment: &str) -> bool {
    let t = segment.trim();
    if t.is_empty() {
        return true;
    }
    if t.contains("\n## ") || t.contains("\n### ") || t.starts_with("## ") || t.starts_with("### ")
    {
        return false;
    }
    true
}

fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    let mut is_first_code_fence = true;
    while i < parts.len() {
        let segment = parts[i];
        i += 1;
        if i >= parts.len() {
            out.push_str(segment);
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            let skip_segment = is_first_code_fence
                && drop_first_segment_before_hidden_agent_reply_plan_fence(segment);
            if !skip_segment {
                out.push_str(segment);
            }
            is_first_code_fence = false;
            continue;
        }
        is_first_code_fence = false;
        out.push_str(segment);
        if unclosed_trailing_fence && i >= parts.len() && inner.trim().is_empty() {
            break;
        }
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

pub(crate) fn assistant_text_for_display(
    raw: &str,
    is_streaming_last_assistant: bool,
    loc: Locale,
    apply_filters: bool,
) -> String {
    if !apply_filters {
        return raw.to_string();
    }
    let inner = assistant_text_for_display_inner(raw, is_streaming_last_assistant, loc);
    filter_assistant_thinking_markers_for_display(&inner, is_streaming_last_assistant)
}

fn assistant_text_for_display_inner(
    raw: &str,
    is_streaming_last_assistant: bool,
    loc: Locale,
) -> String {
    let trimmed = raw.trim();

    if is_streaming_last_assistant && should_buffer_agent_reply_plan_stream(trimmed) {
        // 须与 `assistant_text_for_display` 外套的 `filter_assistant_thinking_markers_for_display` 一致。
        return filter_assistant_thinking_markers_for_display(
            &prose_before_first_fence(trimmed),
            true,
        );
    }

    if let Some(display) = format_agent_reply_plan_json_for_display(trimmed, "", loc)
        && !display.trim().is_empty()
    {
        return display;
    }

    // 无围栏但以前缀 JSON 输出规划：去掉前缀规划对象，保留后续终答正文。
    let t = raw.trim_start();
    if t.starts_with('{') && t.contains("\"agent_reply_plan\"") {
        let mut de = serde_json::Deserializer::from_str(t).into_iter::<Value>();
        if let Some(Ok(v)) = de.next()
            && v.as_object()
                .and_then(|o| o.get("type"))
                .and_then(|x| x.as_str())
                == Some("agent_reply_plan")
        {
            let offset = de.byte_offset();
            if offset < t.len() {
                let tail = t[offset..].trim();
                if !tail.is_empty() {
                    return tail.to_string();
                }
            }
        }
    }

    // 再做一次全量围栏剥离兜底：无论 `agent_reply_plan` 围栏出现在第几个代码块，都不回显原始 JSON。
    let stripped_fences = strip_agent_reply_plan_fence_blocks_for_display(raw);
    let stripped_trim = stripped_fences.trim();
    if stripped_trim != trimmed {
        if stripped_trim.is_empty() && raw.contains("\"agent_reply_plan\"") {
            return crate::i18n::plan_generated(loc).to_string();
        }
        return stripped_trim.to_string();
    }

    raw.to_string()
}

/// 须与主仓 `src/runtime/plan_section.rs` 中 `STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX` 同步。
const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";

/// `role: system` 时间线旁注（分阶段步进）；前缀仅供 UI 分类，展示时剥去。
pub const STAGED_TIMELINE_SYSTEM_PREFIX: &str = "### CrabMate·staged_timeline\n";

pub fn staged_timeline_system_message_body(body: &str) -> String {
    format!("{STAGED_TIMELINE_SYSTEM_PREFIX}{body}")
}

/// Web 聊天列：是否为分阶段实施时间线旁注（`system` + 固定前缀），用于连续多条聚合展示。
#[inline]
pub fn is_staged_timeline_stored_message(m: &StoredMessage) -> bool {
    m.role == "system" && m.text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX)
}

fn user_text_for_chat_display(raw: &str) -> String {
    if raw
        .trim_start()
        .starts_with(STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX)
    {
        return String::new();
    }
    raw.to_string()
}

/// Plain 与行内代码形态（反引号包裹）的 redacted_thinking 开闭标签；与下行 `INLINE_THINKING_OPEN_PREFIXES` 中 redacted 变体对齐。
const REDACTED_LIKE_PAIRS: &[(&str, &str)] = &[
    (
        concat!("<", "redacted", "_", "thinking", ">"),
        concat!("</", "redacted", "_", "thinking", ">"),
    ),
    (
        concat!("`", "<", "redacted", "_", "thinking", ">", "`"),
        concat!("`", "</", "redacted", "_", "thinking", ">", "`"),
    ),
];

/// `<` 或 `</` 之后、大小写不敏感的 `redacted_thinking>`（ASCII）。
const REDACTED_TAG_INNER_ASCII_LOWER: &[u8] = b"redacted_thinking>";

fn bytes_slice_ci_eq_lower(hay: &[u8], i: usize, lower_ascii: &[u8]) -> bool {
    if i + lower_ascii.len() > hay.len() {
        return false;
    }
    for (k, &lb) in lower_ascii.iter().enumerate() {
        if hay[i + k].to_ascii_lowercase() != lb {
            return false;
        }
    }
    true
}

/// 在 `rest` 内找下一处大小写不敏感的 `</redacted_thinking>`，返回相对 `rest` 的字节区间 `[start, end)`。
fn find_ci_plain_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 2 + REDACTED_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<'
            && b[i + 1] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 2, REDACTED_TAG_INNER_ASCII_LOWER)
        {
            let end = i + 2 + REDACTED_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

/// `` ` `<` + 大小写不敏感 `redacted_thinking>` + `` ` ``（与 `REDACTED_LIKE_PAIRS` 中反引号形态对齐，但标签名任意大小写）。
fn find_ci_backtick_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = REDACTED_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 2 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && bytes_slice_ci_eq_lower(b, i + 2, REDACTED_TAG_INNER_ASCII_LOWER)
            && b[i + 2 + inner_len] == b'`'
        {
            return Some((i, i + 2 + inner_len + 1));
        }
        i += 1;
    }
    None
}

/// `` ` `</` + 大小写不敏感 `redacted_thinking>` + `` ` ``。
fn find_ci_backtick_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = REDACTED_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 3 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && b[i + 2] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 3, REDACTED_TAG_INNER_ASCII_LOWER)
            && b[i + 3 + inner_len] == b'`'
        {
            return Some((i, i + 3 + inner_len + 1));
        }
        i += 1;
    }
    None
}

/// 在 `rest` 内找下一处大小写不敏感的 `<redacted_thinking>`，返回相对 `rest` 的字节区间 `[start, end)`。
fn find_ci_plain_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 1 + REDACTED_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<' && bytes_slice_ci_eq_lower(b, i + 1, REDACTED_TAG_INNER_ASCII_LOWER) {
            let end = i + 1 + REDACTED_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

fn find_earliest_redacted_open_span(rest: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (open, _) in REDACTED_LIKE_PAIRS {
        if let Some(rel) = rest.find(open) {
            let end = rel + open.len();
            best = match best {
                None => Some((rel, end)),
                Some((bs, _be)) if rel < bs => Some((rel, end)),
                Some((bs, be)) if rel == bs && end > be => Some((rel, end)),
                o => o,
            };
        }
    }
    if let Some((s, e)) = find_ci_plain_redacted_open_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    if let Some((s, e)) = find_ci_backtick_redacted_open_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    best
}

fn find_earliest_redacted_close_span(rest: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (_, close) in REDACTED_LIKE_PAIRS {
        if let Some(rel) = rest.find(close) {
            let end = rel + close.len();
            best = match best {
                None => Some((rel, end)),
                Some((bs, _be)) if rel < bs => Some((rel, end)),
                Some((bs, be)) if rel == bs && end > be => Some((rel, end)),
                o => o,
            };
        }
    }
    if let Some((s, e)) = find_ci_plain_redacted_close_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    if let Some((s, e)) = find_ci_backtick_redacted_close_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    best
}

/// `streaming == true` 时，去掉 `s` 末尾可能为任一已知开标签前缀的片段，避免半段标签闪烁。
fn strip_trailing_partial_redacted_open(s: &str, streaming: bool) -> &str {
    if !streaming || s.is_empty() {
        return s;
    }
    let b = s.as_bytes();
    let mut longest = 0usize;
    for (open, _) in REDACTED_LIKE_PAIRS {
        let ob = open.as_bytes();
        for k in 1..=ob.len().min(b.len()) {
            if b[b.len() - k..] == ob[..k] {
                longest = longest.max(k);
            }
        }
    }
    // `<` 起头的 CI 开标签前缀：`<`、` <r` … 等
    for k in 1..=(1 + REDACTED_TAG_INNER_ASCII_LOWER.len()).min(b.len()) {
        let start = b.len() - k;
        if b[start] != b'<' {
            continue;
        }
        let after_lt = start + 1;
        let inner_len = b.len() - after_lt;
        if inner_len == 0 {
            longest = longest.max(k);
            continue;
        }
        if bytes_slice_ci_eq_lower(b, after_lt, &REDACTED_TAG_INNER_ASCII_LOWER[..inner_len]) {
            longest = longest.max(k);
        }
    }
    if longest > 0 {
        &s[..s.len() - longest]
    } else {
        s
    }
}

/// 移除 redacted_thinking 块（plain / 反引号包裹 / 开闭标签 ASCII 大小写不敏感）；流式时未闭合开标签之后截断。
pub(crate) fn filter_redacted_thinking_for_display(raw: &str, streaming: bool) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        let rest = &raw[i..];
        let Some((o_start, o_end)) = find_earliest_redacted_open_span(rest) else {
            let tail = strip_trailing_partial_redacted_open(rest, streaming);
            out.push_str(tail);
            break;
        };
        let open_start = i + o_start;
        out.push_str(&raw[i..open_start]);
        let after_open = i + o_end;
        if after_open > raw.len() {
            break;
        }
        let close_rest = &raw[after_open..];
        if let Some((_, c_end)) = find_earliest_redacted_close_span(close_rest) {
            i = after_open + c_end;
            continue;
        }
        if streaming {
            return out;
        }
        return out;
    }
    out
}

/// Qwen / vLLM 等使用的 think 开闭标签（plain 与反引号包裹，标签名大小写不敏感）；与 `INLINE_THINKING_OPEN_PREFIXES` 对齐。
const THINK_LIKE_PAIRS: &[(&str, &str)] = &[
    (concat!("<", "think", ">"), concat!("</", "think", ">")),
    (
        concat!("`", "<", "think", ">", "`"),
        concat!("`", "</", "think", ">", "`"),
    ),
];

/// `<` 或 `</` 之后、大小写不敏感的 `think>`（ASCII）。
const THINK_TAG_INNER_ASCII_LOWER: &[u8] = b"think>";

fn find_ci_plain_think_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 2 + THINK_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<'
            && b[i + 1] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 2, THINK_TAG_INNER_ASCII_LOWER)
        {
            let end = i + 2 + THINK_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

fn find_ci_backtick_think_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = THINK_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 2 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && bytes_slice_ci_eq_lower(b, i + 2, THINK_TAG_INNER_ASCII_LOWER)
            && b[i + 2 + inner_len] == b'`'
        {
            return Some((i, i + 2 + inner_len + 1));
        }
        i += 1;
    }
    None
}

fn find_ci_backtick_think_close_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let inner_len = THINK_TAG_INNER_ASCII_LOWER.len();
    let mut i = 0usize;
    while i + 3 + inner_len < b.len() {
        if b[i] == b'`'
            && b[i + 1] == b'<'
            && b[i + 2] == b'/'
            && bytes_slice_ci_eq_lower(b, i + 3, THINK_TAG_INNER_ASCII_LOWER)
            && b[i + 3 + inner_len] == b'`'
        {
            return Some((i, i + 3 + inner_len + 1));
        }
        i += 1;
    }
    None
}

fn find_ci_plain_think_open_span(rest: &str) -> Option<(usize, usize)> {
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i + 1 + THINK_TAG_INNER_ASCII_LOWER.len() <= b.len() {
        if b[i] == b'<' && bytes_slice_ci_eq_lower(b, i + 1, THINK_TAG_INNER_ASCII_LOWER) {
            let end = i + 1 + THINK_TAG_INNER_ASCII_LOWER.len();
            return Some((i, end));
        }
        i += 1;
    }
    None
}

fn find_earliest_think_open_span(rest: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (open, _) in THINK_LIKE_PAIRS {
        if let Some(rel) = rest.find(open) {
            let end = rel + open.len();
            best = match best {
                None => Some((rel, end)),
                Some((bs, _be)) if rel < bs => Some((rel, end)),
                Some((bs, be)) if rel == bs && end > be => Some((rel, end)),
                o => o,
            };
        }
    }
    if let Some((s, e)) = find_ci_plain_think_open_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    if let Some((s, e)) = find_ci_backtick_think_open_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    best
}

fn find_earliest_think_close_span(rest: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (_, close) in THINK_LIKE_PAIRS {
        if let Some(rel) = rest.find(close) {
            let end = rel + close.len();
            best = match best {
                None => Some((rel, end)),
                Some((bs, _be)) if rel < bs => Some((rel, end)),
                Some((bs, be)) if rel == bs && end > be => Some((rel, end)),
                o => o,
            };
        }
    }
    if let Some((s, e)) = find_ci_plain_think_close_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    if let Some((s, e)) = find_ci_backtick_think_close_span(rest) {
        best = match best {
            None => Some((s, e)),
            Some((bs, _)) if s < bs => Some((s, e)),
            Some((bs, be)) if s == bs && e > be => Some((s, e)),
            o => o,
        };
    }
    best
}

fn strip_trailing_partial_think_open(s: &str, streaming: bool) -> &str {
    if !streaming || s.is_empty() {
        return s;
    }
    let b = s.as_bytes();
    let mut longest = 0usize;
    for (open, _) in THINK_LIKE_PAIRS {
        let ob = open.as_bytes();
        for k in 1..=ob.len().min(b.len()) {
            if b[b.len() - k..] == ob[..k] {
                longest = longest.max(k);
            }
        }
    }
    for k in 1..=(1 + THINK_TAG_INNER_ASCII_LOWER.len()).min(b.len()) {
        let start = b.len() - k;
        if b[start] != b'<' {
            continue;
        }
        let after_lt = start + 1;
        let inner_len = b.len() - after_lt;
        if inner_len == 0 {
            longest = longest.max(k);
            continue;
        }
        if bytes_slice_ci_eq_lower(b, after_lt, &THINK_TAG_INNER_ASCII_LOWER[..inner_len]) {
            longest = longest.max(k);
        }
    }
    if longest > 0 {
        &s[..s.len() - longest]
    } else {
        s
    }
}

/// 移除 `think` 块（plain / 反引号 / 大小写不敏感）；与 `filter_redacted_thinking_for_display` 互补。
fn filter_think_for_display(raw: &str, streaming: bool) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        let rest = &raw[i..];
        let Some((o_start, o_end)) = find_earliest_think_open_span(rest) else {
            let tail = strip_trailing_partial_think_open(rest, streaming);
            out.push_str(tail);
            break;
        };
        let open_start = i + o_start;
        out.push_str(&raw[i..open_start]);
        let after_open = i + o_end;
        if after_open > raw.len() {
            break;
        }
        let close_rest = &raw[after_open..];
        if let Some((_, c_end)) = find_earliest_think_close_span(close_rest) {
            i = after_open + c_end;
            continue;
        }
        if streaming {
            return out;
        }
        return out;
    }
    out
}

/// 助手正文展示：先剥 `redacted_thinking` 再剥 `think`（Qwen 等），二者均支持多段与流式半段前缀；最后压缩连续空行。
pub(crate) fn filter_assistant_thinking_markers_for_display(raw: &str, streaming: bool) -> String {
    let stripped = filter_think_for_display(
        &filter_redacted_thinking_for_display(raw, streaming),
        streaming,
    );
    collapse_consecutive_blank_lines(&stripped)
}

/// 部分网关把思维链塞进 **`content`**，用闭合标记与终答分隔（Qwen / vLLM 等）；与 SSE `reasoning_text` 分轨互补。
const INLINE_THINKING_CLOSE_TAGS: &[&str] = &[
    concat!("</", "think", ">"),
    concat!("</", "redacted", "_", "thinking", ">"),
];

const INLINE_THINKING_OPEN_PREFIXES: &[&str] = &[
    concat!("`", "<", "think", ">", "`"),
    concat!("`<", "think", ">"),
    concat!("<", "think", ">"),
    concat!("`", "<", "redacted", "_", "thinking", ">", "`"),
    concat!("`<", "redacted", "_", "thinking", ">"),
    concat!("<", "redacted", "_", "thinking", ">"),
];

fn first_inline_thinking_close(raw: &str) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;
    for tag in INLINE_THINKING_CLOSE_TAGS {
        if let Some(i) = raw.find(tag) {
            best = match best {
                None => Some((i, *tag)),
                Some((bi, _bt)) if i < bi => Some((i, *tag)),
                Some((bi, bt)) if i == bi && tag.len() > bt.len() => Some((i, *tag)),
                o => o,
            };
        }
    }
    best
}

fn trim_inline_thinking_openers(mut s: &str) -> &str {
    s = s.trim();
    loop {
        let mut stripped = false;
        for pre in INLINE_THINKING_OPEN_PREFIXES {
            if let Some(rest) = s.strip_prefix(pre) {
                s = rest.trim();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    s
}

/// 优先已存 `reasoning_text`；否则尝试从 `text` 中按内联闭合标记拆出思维链与终答原文（供 Markdown 与折叠长度用）。
pub(crate) fn assistant_thinking_body_and_answer_raw<'a>(
    reasoning_text_stored: &'a str,
    text_stored: &'a str,
    split_inline_thinking: bool,
) -> (&'a str, &'a str) {
    let rs = reasoning_text_stored.trim();
    if !rs.is_empty() {
        return (rs, text_stored);
    }
    if !split_inline_thinking {
        return ("", text_stored);
    }
    let Some((idx, tag)) = first_inline_thinking_close(text_stored) else {
        return ("", text_stored);
    };
    let after = text_stored[idx + tag.len()..].trim_start();
    if after.is_empty() {
        return ("", text_stored);
    }
    let thinking = trim_inline_thinking_openers(&text_stored[..idx]);
    (thinking, after)
}

/// `apply_assistant_display_filters == false` 时助手消息按存储原文输出（不剥 `agent_reply_plan`、不拆内联思维链标记）。
pub fn message_text_for_display_ex(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if m.role == "assistant" {
        let is_streaming_last_assistant = m.state.as_deref() == Some("loading");
        let reasoning_for_split: Cow<str> = if apply_assistant_display_filters {
            Cow::Owned(filter_assistant_thinking_markers_for_display(
                m.reasoning_text.as_str(),
                is_streaming_last_assistant,
            ))
        } else {
            Cow::Borrowed(m.reasoning_text.as_str())
        };
        let text_for_split: Cow<str> = if apply_assistant_display_filters {
            Cow::Owned(filter_assistant_thinking_markers_for_display(
                m.text.as_str(),
                is_streaming_last_assistant,
            ))
        } else {
            Cow::Borrowed(m.text.as_str())
        };
        let (r_body, t_body) = assistant_thinking_body_and_answer_raw(
            reasoning_for_split.as_ref(),
            text_for_split.as_ref(),
            apply_assistant_display_filters,
        );
        let answer = assistant_text_for_display(
            t_body,
            is_streaming_last_assistant,
            loc,
            apply_assistant_display_filters,
        );
        if apply_assistant_display_filters {
            let r = r_body.trim();
            if r.is_empty() {
                answer
            } else if answer.trim().is_empty() {
                r.to_string()
            } else {
                format!("{r}\n\n{answer}")
            }
        } else {
            let r_empty = r_body.trim().is_empty();
            let a_empty = answer.trim().is_empty();
            if r_empty {
                answer
            } else if a_empty {
                r_body.to_string()
            } else {
                format!("{r_body}\n\n{answer}")
            }
        }
    } else if m.role == "user" {
        user_text_for_chat_display(&m.text)
    } else if m.role == "system" {
        m.text
            .strip_prefix(STAGED_TIMELINE_SYSTEM_PREFIX)
            .unwrap_or(m.text.as_str())
            .to_string()
    } else {
        m.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;
    use super::assistant_text_for_display;
    use super::assistant_thinking_body_and_answer_raw;
    use super::collapse_consecutive_blank_lines;
    use super::filter_assistant_thinking_markers_for_display;
    use super::filter_redacted_thinking_for_display;
    use super::message_text_for_display_ex;
    use crate::i18n::Locale;
    use crate::storage::StoredMessage;

    #[test]
    fn collapse_consecutive_blank_lines_merges_runs() {
        assert_eq!(collapse_consecutive_blank_lines("a\n\n\nb"), "a\n\nb");
        assert_eq!(collapse_consecutive_blank_lines("\n\nfoo"), "foo");
        assert_eq!(collapse_consecutive_blank_lines("x\n  \n\t\ny"), "x\n\ny");
    }

    #[test]
    fn hide_inline_agent_reply_plan_json_fence() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```"#;
        let out = assistant_text_for_display(raw, true, Locale::ZhHans, true);
        assert!(
            !out.contains("agent_reply_plan"),
            "raw agent_reply_plan json should be filtered: {out}"
        );
        assert!(
            !out.contains("```"),
            "agent_reply_plan fence should be stripped: {out}"
        );
    }

    #[test]
    fn no_task_empty_plan_has_non_empty_fallback() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            !out.trim().is_empty(),
            "filtered plan text should not become empty"
        );
    }

    #[test]
    fn keep_answer_after_fenced_plan_json() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```最终结论：已完成。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn keep_answer_after_unfenced_plan_json_prefix() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}最终结论：继续执行。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn drops_prose_before_first_agent_reply_plan_fence() {
        let preamble = "模型规划说明（不应展示）\n\n";
        let raw = format!(
            r#"{preamble}```json{{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}}```最终结论：保留。"#,
            preamble = preamble
        );
        let out = assistant_text_for_display(&raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail after fence should be kept: {out}"
        );
        assert!(
            !out.contains("模型规划说明"),
            "preamble before first plan fence should be dropped: {out}"
        );
    }

    #[test]
    fn strips_qwen_think_block_in_combined_filter() {
        let raw = concat!(
            "你好",
            "<",
            "think",
            ">",
            "内省正文",
            "</",
            "think",
            ">",
            "尾",
        );
        let out = filter_assistant_thinking_markers_for_display(raw, false);
        assert_eq!(out, "你好尾");
    }

    #[test]
    fn strips_two_think_blocks_in_combined_filter() {
        let o = concat!("<", "think", ">");
        let c = concat!("</", "think", ">");
        let raw = format!("a{o}1{c}m{o}2{c}z");
        let out = filter_assistant_thinking_markers_for_display(&raw, false);
        assert_eq!(out, "amz");
    }

    fn assert_filtered_redacted_plan_export_body(out: &str) {
        let open = concat!("<", "redacted", "_", "thinking", ">");
        let close = concat!("</", "redacted", "_", "thinking", ">");
        assert!(
            !out.contains(open),
            "redacted open tag should be stripped:\n{out}"
        );
        assert!(
            !out.contains(close),
            "redacted close tag should be stripped:\n{out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "plan json should be hidden:\n{out}"
        );
        assert!(
            !out.contains("用户问"),
            "first redacted block body should be removed:\n{out}"
        );
        assert!(
            !out.contains("用户发送了"),
            "second redacted block body should be removed:\n{out}"
        );
        assert!(
            out.contains("CrabMate"),
            "visible prose should remain:\n{out}"
        );
        assert!(
            out.contains("有具体代码任务"),
            "tail prose before fence should remain:\n{out}"
        );
        assert!(
            out.contains("好的，我可以帮你"),
            "final answer line should remain:\n{out}"
        );
    }

    /// 工作区根目录 `chat_selection_20260410_230651.md`（可选）：与 `chat_resp1` 同形，但带 `## 助手` 导出标题；文件不存在时跳过。
    #[test]
    fn filter_chat_selection_export_fixture_md() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../chat_selection_20260410_230651.md");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return;
        };
        let body = raw
            .strip_prefix("## 助手\n\n")
            .or_else(|| raw.strip_prefix("## 助手\r\n\r\n"))
            .unwrap_or(raw.as_str());
        let out = assistant_text_for_display(body, false, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    /// `fixtures/chat_resp1.md`：助手原文示例（两段 `<redacted_thinking>` + 文末 `agent_reply_plan` 围栏），供 `assistant_text_for_display` 过滤回归。
    #[test]
    fn filter_chat_resp1_fixture_md() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat_resp1.md");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let out = assistant_text_for_display(raw.trim(), false, Locale::ZhHans, true);
        assert_filtered_redacted_plan_export_body(&out);
    }

    #[test]
    fn no_inline_split_when_disabled() {
        let raw = concat!("<", "think", ">", "x", "</", "think", ">", "y",);
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, false);
        assert!(think.is_empty());
        assert_eq!(ans, raw);
    }

    #[test]
    fn assistant_text_passthrough_when_filters_off() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, false);
        assert_eq!(out, raw);
    }

    #[test]
    fn splits_inline_thinking_from_assistant_content_when_no_reasoning_field() {
        let raw = concat!(
            "<",
            "think",
            ">",
            "plan here",
            "</",
            "think",
            ">",
            "\n\n**Answer** tail.",
        );
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, true);
        assert_eq!(think.trim(), "plan here");
        assert!(ans.contains("Answer"));
        assert!(!ans.contains("plan here"));
    }

    #[test]
    fn stored_reasoning_text_wins_over_inline_tags() {
        let inline = concat!("`<", "think", ">`x`</", "think", ">`y");
        let (think, ans) = assistant_thinking_body_and_answer_raw("from_sse", inline, true);
        assert_eq!(think, "from_sse");
        assert_eq!(ans, inline);
    }

    #[test]
    fn strips_redacted_thinking_pair_complete() {
        let raw = concat!(
            "pre ", "<", "redacted", "_", "thinking", ">", "hidden", "</", "redacted", "_",
            "thinking", ">", " tail",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "pre  tail");
    }

    #[test]
    fn strips_two_redacted_thinking_pairs() {
        let o = concat!("<", "redacted", "_", "thinking", ">");
        let c = concat!("</", "redacted", "_", "thinking", ">");
        let raw = format!("a{o}x{c} b{o}y{c} c");
        let out = filter_redacted_thinking_for_display(&raw, false);
        assert_eq!(out, "a b c");
    }

    #[test]
    fn redacted_streaming_truncates_before_unclosed_block() {
        let raw = concat!("ok", "<", "redacted", "_", "thinking", ">", "partial",);
        let out = filter_redacted_thinking_for_display(raw, true);
        assert_eq!(out, "ok");
    }

    #[test]
    fn redacted_streaming_strips_suffix_matching_open_prefix() {
        let raw = "visible<redacted_thin";
        let out = filter_redacted_thinking_for_display(raw, true);
        assert_eq!(out, "visible");
    }

    #[test]
    fn strips_backtick_wrapped_redacted_pair() {
        let raw = concat!(
            "x", "`", "<", "redacted", "_", "thinking", ">", "`", "h", "`", "</", "redacted", "_",
            "thinking", ">", "`", "y",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "xy");
    }

    #[test]
    fn strips_case_insensitive_redacted_tags() {
        let raw = "<Redacted_Thinking>sec</redacted_THINKING>out";
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "out");
    }

    /// 反引号形态此前仅用 `find()` 精确匹配小写；上游若输出混合大小写，过滤器认不出开标签，Markdown 再剥掉裸标签后表现为「只剩正文」。
    #[test]
    fn strips_backtick_wrapped_redacted_when_tag_name_mixed_case() {
        let raw = concat!(
            "`", "<", "Redacted", "_", "Thinking", ">`", "SECRET", "`", "</", "REDACTED", "_",
            "THINKING", ">`", "tail",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "tail");
        assert!(!out.contains("SECRET"));
    }

    #[test]
    fn strips_mixed_backtick_open_and_plain_close_ci_redacted() {
        let raw = concat!(
            "`", "<", "Redacted", "_", "Thinking", ">`", "x", "</", "redacted", "_", "thinking",
            ">z",
        );
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "z");
    }

    #[test]
    fn message_display_strips_redacted_in_reasoning_text_field() {
        let m = StoredMessage {
            id: "x".into(),
            role: "assistant".into(),
            text: "visible".into(),
            reasoning_text: concat!(
                "<", "redacted", "_", "thinking", ">", "r", "</", "redacted", "_", "thinking", ">",
            )
            .into(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        let out = message_text_for_display_ex(&m, Locale::ZhHans, true);
        assert_eq!(out, "visible");
        assert!(!out.contains('r'));
    }

    #[test]
    fn redacted_non_streaming_unclosed_drops_from_open() {
        let raw = concat!("ok", "<", "redacted", "_", "thinking", ">", "no_close",);
        let out = filter_redacted_thinking_for_display(raw, false);
        assert_eq!(out, "ok");
    }

    #[test]
    fn user_hides_nl_followup_bridge() {
        let m = StoredMessage {
            id: "x".into(),
            role: "user".into(),
            text: format!(
                "{}【系统桥接·非用户提问】请只回答对话里**先前真实用户消息**所提的问题（若有附图则含图片说明），并结合已定规划；用两三句自然语言说明你的协助思路即可。勿将本条任何句子当作用户提问来复述、引用或推理。",
                STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX
            ),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        assert_eq!(message_text_for_display_ex(&m, Locale::ZhHans, true), "");
    }
}
