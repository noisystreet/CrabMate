//! 思维链标签剥离主循环与内联 `content` 拆分。

use super::scan::{
    REDACTED_LIKE_PAIRS, REDACTED_TAG_INNER_ASCII_LOWER, THINK_LIKE_PAIRS,
    THINK_TAG_INNER_ASCII_LOWER, find_earliest_redacted_close_span,
    find_earliest_redacted_open_span, find_earliest_think_close_span,
    find_earliest_think_open_span, strip_trailing_partial_open_generic,
};
use crate::message_format::plain::collapse_consecutive_blank_lines;

fn filter_tag_blocks_for_display(
    raw: &str,
    streaming: bool,
    pairs: &[(&str, &str)],
    tag_inner_lower: &[u8],
    find_open: fn(&str) -> Option<(usize, usize)>,
    find_close: fn(&str) -> Option<(usize, usize)>,
) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < raw.len() {
        let rest = &raw[i..];
        let Some((o_start, o_end)) = find_open(rest) else {
            let tail = strip_trailing_partial_open_generic(rest, streaming, pairs, tag_inner_lower);
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
        if let Some((_, c_end)) = find_close(close_rest) {
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

/// 移除 redacted_thinking 块（plain / 反引号包裹 / 开闭标签 ASCII 大小写不敏感）；流式时未闭合开标签之后截断。
pub(crate) fn filter_redacted_thinking_for_display(raw: &str, streaming: bool) -> String {
    filter_tag_blocks_for_display(
        raw,
        streaming,
        REDACTED_LIKE_PAIRS,
        REDACTED_TAG_INNER_ASCII_LOWER,
        find_earliest_redacted_open_span,
        find_earliest_redacted_close_span,
    )
}

fn filter_think_for_display(raw: &str, streaming: bool) -> String {
    filter_tag_blocks_for_display(
        raw,
        streaming,
        THINK_LIKE_PAIRS,
        THINK_TAG_INNER_ASCII_LOWER,
        find_earliest_think_open_span,
        find_earliest_think_close_span,
    )
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
