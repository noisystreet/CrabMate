//! `think` / `redacted_thinking` 标签剥离与内联思维链拆分（与 `plan_fence` 互补）。

use super::super::plain::collapse_consecutive_blank_lines;

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
