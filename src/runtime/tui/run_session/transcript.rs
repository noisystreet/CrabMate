//! TUI 中区 transcript：与 Web 快照一致的过滤与 [`crate::runtime::message_display`] 展示路径
//!（工具条优先 `crabmate-tool-card` compact，与 Tauri/Web 同源）。
//!
//! **定稿投影（Phase 1）**：含工具的回合结束后将 `project_turn_web_v2` 行序写入
//! [`CommittedTurns`]，避免下一轮 `turn_projection.reset` 后历史退回 OpenAI 落盘序。

use crate::runtime::message_display::{
    assistant_markdown_source_for_message, tool_content_for_display_for_message,
    user_message_for_chat_display,
};
use crate::text_util::truncate_chars_with_ellipsis;
use crate::types::{
    Message, is_message_visible_in_chat_transcript, message_content_as_str,
    message_content_plain_for_chat_display,
};

use super::turn_project::TuiTurnProjection;

/// 已定稿回合的展示文本与覆盖的 `messages` 前缀长度。
#[derive(Debug, Clone, Default)]
pub(super) struct CommittedTurns {
    pub(super) display: String,
    pub(super) msg_len: usize,
}

impl CommittedTurns {
    pub(super) fn reseed_from_messages(messages: &[Message]) -> Self {
        Self {
            display: messages_to_transcript(messages),
            msg_len: messages.len(),
        }
    }

    /// 会话消息被替换（`/conv open` 等）或长度不一致时重建。
    pub(super) fn ensure_consistent_with(&mut self, messages: &[Message]) {
        if self.msg_len != messages.len() {
            *self = Self::reseed_from_messages(messages);
        }
    }

    /// 把本轮投影（或无投影时的 Message[] 切片）并入定稿，推进 `msg_len`。
    pub(super) fn flush_completed_turn(
        &mut self,
        messages: &[Message],
        projection: &TuiTurnProjection,
    ) {
        let turn_start = self.msg_len.min(messages.len());
        let piece = format_completed_turn_for_past_display(messages, turn_start, projection);
        if !piece.is_empty() {
            if !self.display.is_empty() && !self.display.ends_with('\n') {
                self.display.push('\n');
            }
            self.display.push_str(&piece);
        }
        self.msg_len = messages.len();
        const MAX_CHARS: usize = 96_000;
        if self.display.len() > MAX_CHARS {
            let drain = self.display.len() - MAX_CHARS;
            let safe = next_char_boundary(&self.display, drain);
            self.display.drain(..safe);
        }
    }
}

/// 定稿前缀 + 尚未 flush 的消息尾（通常为本轮 user）。
pub(super) fn transcript_with_in_progress(
    committed: &CommittedTurns,
    messages: &[Message],
) -> String {
    if committed.msg_len >= messages.len() {
        return committed.display.clone();
    }
    let rest = messages_to_transcript_range(messages, committed.msg_len, messages.len());
    if rest.is_empty() {
        return committed.display.clone();
    }
    let mut out = committed.display.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&rest);
    out
}

pub(super) fn messages_to_transcript(messages: &[Message]) -> String {
    const MAX_TAIL: usize = 48;
    let visible: Vec<(usize, &Message)> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| is_message_visible_in_chat_transcript(m))
        .collect();
    let start = visible.len().saturating_sub(MAX_TAIL);
    let mut out = String::new();
    for (idx, _) in visible.into_iter().skip(start) {
        append_visible_message_block(&mut out, messages, idx);
    }
    const MAX_CHARS: usize = 96_000;
    if out.len() > MAX_CHARS {
        let drain = out.len() - MAX_CHARS;
        let safe = next_char_boundary(&out, drain);
        out.drain(..safe);
    }
    out
}

fn messages_to_transcript_range(messages: &[Message], start: usize, end: usize) -> String {
    let end = end.min(messages.len());
    let start = start.min(end);
    let mut out = String::new();
    for idx in start..end {
        if !is_message_visible_in_chat_transcript(&messages[idx]) {
            continue;
        }
        append_visible_message_block(&mut out, messages, idx);
    }
    out
}

/// 有投影：user/前缀 → `[Turn 投影]`（旁白在工具前）→ 终答等后缀；无投影：整段 Message[]。
fn format_completed_turn_for_past_display(
    messages: &[Message],
    turn_start: usize,
    projection: &TuiTurnProjection,
) -> String {
    let block = projection.format_projection_block();
    if block.is_empty() {
        return messages_to_transcript_range(messages, turn_start, messages.len());
    }

    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut seen_tool_phase = false;
    let end = messages.len();
    for idx in turn_start..end {
        let m = &messages[idx];
        if !is_message_visible_in_chat_transcript(m) {
            continue;
        }
        let role = m.role.as_str();
        let is_tc_assistant = role == "assistant" && assistant_has_tool_calls(m);
        let is_tool = role == "tool";
        if is_tool || is_tc_assistant {
            seen_tool_phase = true;
            continue;
        }
        let body = message_body_for_transcript(messages, idx);
        if body.is_empty() {
            continue;
        }
        let chunk = format!("[{role}]\n{body}\n\n");
        if seen_tool_phase {
            suffix.push_str(&chunk);
        } else {
            prefix.push_str(&chunk);
        }
    }

    let mut out = prefix;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&block);
    if !block.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&suffix);
    out
}

fn append_visible_message_block(out: &mut String, messages: &[Message], msg_idx: usize) {
    let body = message_body_for_transcript(messages, msg_idx);
    if body.is_empty() {
        return;
    }
    out.push_str(&format!("[{}]\n{}\n\n", messages[msg_idx].role, body));
}

fn assistant_has_tool_calls(m: &Message) -> bool {
    m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tui::TuiLlmStreamScratch;
    use crate::sse::{SsePayload, ToolCallSummary};
    use crate::types::{FunctionCall, Message, MessageContent, ToolCall};

    fn user_msg(text: &str) -> Message {
        Message::user_only(text)
    }

    fn assistant_with_tools(text: &str, name: &str) -> Message {
        let mut m = Message::assistant_only(text);
        m.tool_calls = Some(vec![ToolCall {
            id: "tc1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: "{}".into(),
            },
        }]);
        m
    }

    fn tool_msg(text: &str) -> Message {
        Message {
            role: "tool".into(),
            content: Some(MessageContent::Text(text.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("read_file".into()),
            tool_call_id: Some("tc1".into()),
        }
    }

    #[test]
    fn flush_keeps_commentary_before_tool_after_projection_reset() {
        let mut committed = CommittedTurns::default();
        let mut messages = vec![user_msg("分析项目")];
        let mut proj = TuiTurnProjection::default();
        let scratch = TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        proj.apply_sse(
            &SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            },
            &scratch,
        );
        proj.apply_sse(
            &SsePayload::ToolCall {
                tool_call: ToolCallSummary {
                    name: "read_file".into(),
                    summary: "README.md".into(),
                    goal_id: None,
                    tool_call_id: Some("tc1".into()),
                    arguments_preview: None,
                    arguments: None,
                },
            },
            &scratch,
        );
        messages.push(assistant_with_tools("先看一下 README。", "read_file"));
        messages.push(tool_msg("ok"));
        messages.push(Message::assistant_only("总结如下。"));

        proj.finalize_for_display(&TuiLlmStreamScratch::default());
        committed.flush_completed_turn(&messages, &proj);
        proj.reset();

        assert!(
            proj.format_projection_block().is_empty(),
            "live projection must be empty after reset"
        );
        let display = committed.display.as_str();
        let commentary = display.find("[旁白]").expect("旁白");
        let tool = display.find("[工具 · read_file]").expect("工具");
        assert!(commentary < tool, "旁白须在工具之前: {display}");
        assert!(display.contains("[user]"), "{display}");
        assert!(display.contains("总结如下"), "{display}");
        // 投影承接工具后，Message[] 的 tool 行不应再出现在定稿里
        assert!(
            !display.contains("[tool]"),
            "tool role must not duplicate projection: {display}"
        );
        assert_eq!(committed.msg_len, messages.len());
    }

    #[test]
    fn reseed_when_message_len_diverges() {
        let mut committed = CommittedTurns {
            display: "stale".into(),
            msg_len: 99,
        };
        let messages = vec![user_msg("hi")];
        committed.ensure_consistent_with(&messages);
        assert_eq!(committed.msg_len, 1);
        assert!(committed.display.contains("[user]"));
        assert!(committed.display.contains("hi"));
    }
}
