//! 从持久化 `Message[]` 还原 canonical 段键，供会话 `layout` 落盘（B2 PR3）。
//!
//! 与流式 SSE 同用 [`TurnReducer`] + [`project_turn_web_v2`]；不是 B3 hydration 读路径。
//! 终答不经 reducer 关段（关段会丢正文），按消息顺序在投影行后追加。

use crate::cm_api_contract::chat::{
    CONVERSATION_LAYOUT_SCHEMA_VERSION_V2, ConversationLayoutMeta, ConversationLayoutSegment,
};
use crate::cm_turn_layout::event::TurnEvent;
use crate::cm_turn_layout::model::{SegmentKind, Turn};
use crate::cm_turn_layout::project::{ASSISTANT_ANSWER, ProjectedRow, project_turn_web_v2};
use crate::cm_turn_layout::reduce::TurnReducer;
use crate::cm_types::{
    Message, ToolCall, is_chat_timeline_marker, message_content_plain_for_chat_display,
    user_message_counts_for_branch_truncation,
};

/// 由当前落盘消息生成会话级布局元数据（始终带 `layout_schema_version`）。
#[must_use]
pub fn layout_meta_from_messages(messages: &[Message]) -> ConversationLayoutMeta {
    let (segments, rows) = segments_and_rows_from_messages(messages);
    let rows_json = serde_json::to_vec(&rows).unwrap_or_else(|_| b"[]".to_vec());
    ConversationLayoutMeta {
        layout_schema_version: CONVERSATION_LAYOUT_SCHEMA_VERSION_V2,
        projection_hash: Some(format!("{:016x}", fnv1a64(&rows_json))),
        segments,
    }
}

fn segments_and_rows_from_messages(
    messages: &[Message],
) -> (Vec<ConversationLayoutSegment>, Vec<ProjectedRow>) {
    let mut segments = Vec::new();
    let mut all_rows = Vec::new();
    let mut sequence = 0u32;
    for (ordinal, slice) in iter_user_turn_slices(messages) {
        let turn_id = format!("u{ordinal}");
        for row in rows_for_turn_slice(slice) {
            let segment_id = row
                .tool_call_id
                .clone()
                .unwrap_or_else(|| format!("{}-{sequence}", row.kind));
            segments.push(ConversationLayoutSegment {
                turn_id: Some(turn_id.clone()),
                segment_id,
                segment_kind: row.kind.clone(),
                before_tool_call_id: row.tool_call_id.clone(),
                sequence,
            });
            all_rows.push(row);
            sequence = sequence.saturating_add(1);
        }
    }
    (segments, all_rows)
}

fn rows_for_turn_slice(slice: &[Message]) -> Vec<ProjectedRow> {
    let mut turn = Turn::default();
    let reducer = TurnReducer;
    let mut rows = Vec::new();
    let mut v2_emitted = 0usize;
    let mut saw_tools = false;
    for m in slice {
        if is_chat_timeline_marker(m) {
            apply_timeline(&reducer, &mut turn, m);
            flush_new_v2_rows(&turn, &mut rows, &mut v2_emitted);
            continue;
        }
        if m.role != "assistant" {
            continue;
        }
        let tools = m.tool_calls.as_deref().unwrap_or(&[]);
        if tools.is_empty() {
            if saw_tools {
                reducer.apply(&mut turn, TurnEvent::ToolPhaseEnd);
                saw_tools = false;
                flush_new_v2_rows(&turn, &mut rows, &mut v2_emitted);
            }
            push_answer_row(&mut rows, m);
        } else {
            apply_tool_batch(&reducer, &mut turn, m, tools);
            saw_tools = true;
            flush_new_v2_rows(&turn, &mut rows, &mut v2_emitted);
        }
    }
    if saw_tools {
        reducer.apply(&mut turn, TurnEvent::ToolPhaseEnd);
        flush_new_v2_rows(&turn, &mut rows, &mut v2_emitted);
    }
    crate::cm_turn_layout::close_open_commentary_segments(&mut turn);
    flush_new_v2_rows(&turn, &mut rows, &mut v2_emitted);
    rows
}

fn flush_new_v2_rows(turn: &Turn, rows: &mut Vec<ProjectedRow>, v2_emitted: &mut usize) {
    let v2 = project_turn_web_v2(turn);
    if v2.len() > *v2_emitted {
        rows.extend(v2[*v2_emitted..].iter().cloned());
        *v2_emitted = v2.len();
    }
}

fn apply_timeline(reducer: &TurnReducer, turn: &mut Turn, m: &Message) {
    let text = message_content_plain_for_chat_display(&m.content);
    if text.trim().is_empty() {
        return;
    }
    reducer.apply(turn, TurnEvent::TimelineAssistant { text });
}

fn apply_tool_batch(reducer: &TurnReducer, turn: &mut Turn, m: &Message, tools: &[ToolCall]) {
    let text = message_content_plain_for_chat_display(&m.content);
    if !text.trim().is_empty()
        && let Some(first) = tools.first()
    {
        let seg_id = format!("seg-before-{}", first.id);
        reducer.apply(
            turn,
            TurnEvent::SegmentStart {
                segment_id: seg_id.clone(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: Some(first.id.clone()),
            },
        );
        reducer.apply(
            turn,
            TurnEvent::SegmentDelta {
                segment_id: seg_id.clone(),
                delta: text,
            },
        );
        reducer.apply(turn, TurnEvent::SegmentEnd { segment_id: seg_id });
    }
    for tc in tools {
        reducer.apply(
            turn,
            TurnEvent::ToolCall {
                tool_call_id: tc.id.clone(),
                name: tc.function.name.clone(),
                summary: tc.function.name.clone(),
            },
        );
    }
}

fn push_answer_row(rows: &mut Vec<ProjectedRow>, m: &Message) {
    let text = message_content_plain_for_chat_display(&m.content);
    if text.trim().is_empty() {
        return;
    }
    rows.push(ProjectedRow {
        kind: ASSISTANT_ANSWER.into(),
        text,
        tool_name: None,
        tool_call_id: None,
    });
}

fn iter_user_turn_slices(messages: &[Message]) -> Vec<(u32, &[Message])> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut ordinal = 0u32;
    while i < messages.len() {
        if user_message_counts_for_branch_truncation(&messages[i]) {
            let start = i.saturating_add(1);
            i = start;
            while i < messages.len() && !user_message_counts_for_branch_truncation(&messages[i]) {
                i = i.saturating_add(1);
            }
            out.push((ordinal, &messages[start..i]));
            ordinal = ordinal.saturating_add(1);
        } else {
            i = i.saturating_add(1);
        }
    }
    out
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::layout_meta_from_messages;
    use crate::cm_types::{FunctionCall, Message, MessageContent, ToolCall};

    fn user(text: &str) -> Message {
        Message::user_only(text.to_string())
    }

    fn assistant_text(text: &str) -> Message {
        Message {
            role: "assistant".into(),
            content: Some(MessageContent::Text(text.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn assistant_tools(text: &str, id: &str, name: &str) -> Message {
        Message {
            role: "assistant".into(),
            content: Some(MessageContent::Text(text.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: id.into(),
                typ: "function".into(),
                function: FunctionCall {
                    name: name.into(),
                    arguments: "{}".into(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn empty_messages_still_versioned() {
        let meta = layout_meta_from_messages(&[]);
        assert_eq!(meta.layout_schema_version, 2);
        assert!(meta.segments.is_empty());
        assert!(meta.projection_hash.is_some());
    }

    #[test]
    fn tool_then_answer_emits_commentary_tool_and_answer_kinds() {
        let msgs = vec![
            user("go"),
            assistant_tools("先读。", "tc1", "read_file"),
            assistant_text("完毕。"),
        ];
        let meta = layout_meta_from_messages(&msgs);
        let kinds: Vec<_> = meta
            .segments
            .iter()
            .map(|s| s.segment_kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec!["assistant_commentary", "tool", "assistant_answer"]
        );
        assert_eq!(meta.segments[0].turn_id.as_deref(), Some("u0"));
        assert_eq!(meta.segments[1].before_tool_call_id.as_deref(), Some("tc1"));
        assert_eq!(meta.segments[1].segment_id, "tc1");
    }

    #[test]
    fn interleaved_answer_stays_between_tool_batches() {
        let msgs = vec![
            user("go"),
            assistant_tools("一。", "tc1", "read_file"),
            assistant_text("中段。"),
            assistant_tools("二。", "tc2", "list_tree"),
        ];
        let meta = layout_meta_from_messages(&msgs);
        let kinds: Vec<_> = meta
            .segments
            .iter()
            .map(|s| s.segment_kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "assistant_commentary",
                "tool",
                "assistant_answer",
                "assistant_commentary",
                "tool"
            ]
        );
    }

    #[test]
    fn hash_stable_for_same_messages() {
        let msgs = vec![user("a"), assistant_text("b")];
        let a = layout_meta_from_messages(&msgs);
        let b = layout_meta_from_messages(&msgs);
        assert_eq!(a.projection_hash, b.projection_hash);
        assert_eq!(a.segments.len(), 1);
        assert_eq!(a.segments[0].segment_kind, "assistant_answer");
    }
}
