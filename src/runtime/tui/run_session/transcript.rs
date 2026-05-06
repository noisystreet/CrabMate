//! TUI 中区 transcript：与 Web 快照一致的过滤与 [`crate::runtime::message_display`] 展示路径。

use crate::runtime::message_display::{
    assistant_markdown_source_for_message, tool_content_for_display_for_message,
    user_message_for_chat_display,
};
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::{
    Message, is_message_visible_in_chat_transcript, message_content_as_str,
    message_content_plain_for_chat_display,
};

pub(super) fn messages_to_transcript(messages: &[Message]) -> String {
    const MAX_TAIL: usize = 48;
    let visible: Vec<(usize, &Message)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| is_message_visible_in_chat_transcript(m))
        .collect();
    let start = visible.len().saturating_sub(MAX_TAIL);
    let mut out = String::new();
    for (idx, m) in visible.into_iter().skip(start) {
        let body = message_body_for_transcript(messages, idx);
        if body.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}]\n{}\n\n", m.role, body));
    }
    const MAX_CHARS: usize = 96_000;
    if out.len() > MAX_CHARS {
        let drain = out.len() - MAX_CHARS;
        let safe = next_char_boundary(&out, drain);
        out.drain(..safe);
    }
    out
}

fn message_body_for_transcript(messages: &[Message], msg_idx: usize) -> String {
    let m = &messages[msg_idx];
    match m.role.as_str() {
        "assistant" => {
            let body = assistant_markdown_source_for_message(m);
            let t = body.trim();
            if t.is_empty() {
                String::new()
            } else {
                truncate_chars_with_ellipsis(t, 12_000)
            }
        }
        "user" => {
            let plain = message_content_plain_for_chat_display(&m.content);
            let shown = user_message_for_chat_display(&plain);
            let t = shown.trim();
            if t.is_empty() {
                String::new()
            } else {
                truncate_chars_with_ellipsis(t, 8000)
            }
        }
        "tool" => {
            let body = if let Some(raw) = message_content_as_str(&m.content) {
                tool_content_for_display_for_message(raw, messages, msg_idx)
            } else {
                message_content_plain_for_chat_display(&m.content)
            };
            let t = body.trim();
            if t.is_empty() {
                String::new()
            } else {
                truncate_chars_with_ellipsis(t, 8000)
            }
        }
        _ => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(r) = m
                .reasoning_content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(format!("(推理) {}", truncate_chars_with_ellipsis(r, 2000)));
            }
            let plain = message_content_plain_for_chat_display(&m.content);
            let trimmed = plain.trim();
            if !trimmed.is_empty() {
                parts.push(truncate_chars_with_ellipsis(trimmed, 8000));
            }
            if parts.is_empty() {
                String::new()
            } else {
                parts.join("\n")
            }
        }
    }
}

fn next_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut i = byte_idx.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}
