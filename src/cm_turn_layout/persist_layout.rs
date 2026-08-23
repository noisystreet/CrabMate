//! 从持久化 `Message[]` 还原 canonical 段键，供会话 `layout` 落盘（B2 PR3）。
//!
//! 每个用户回合：先 [`TurnReducer`] 吃完时间线/工具，再 **一次** [`project_turn_web_v2`]
//!（时间线固定在工具前，与流式投影一致）；终答不经 reducer 关段（关段会丢正文），
//! 按「当时已声明工具步数」插入。首条计次用户之前的时间线归 `turn_id=lead`。

use crate::cm_api_contract::chat::{
    CONVERSATION_LAYOUT_SCHEMA_VERSION_V2, ConversationLayoutMeta, ConversationLayoutSegment,
};
use crate::cm_turn_layout::event::TurnEvent;
use crate::cm_turn_layout::model::{SegmentKind, Turn};
use crate::cm_turn_layout::project::{
    ASSISTANT_ANSWER, ASSISTANT_COMMENTARY, ProjectedRow, project_turn_web_v2,
};
use crate::cm_turn_layout::reduce::TurnReducer;
use crate::cm_types::{
    Message, ToolCall, is_chat_timeline_marker, message_content_plain_for_chat_display,
    user_message_counts_for_branch_truncation,
};

const TOOL_KIND: &str = "tool";
const LEAD_TURN_ID: &str = "lead";

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
    for (turn_id, slice) in iter_turn_slices(messages) {
        for row in rows_for_turn_slice(slice) {
            let (segment_id, before_tool_call_id) = segment_keys(&row, sequence);
            segments.push(ConversationLayoutSegment {
                turn_id: Some(turn_id.clone()),
                segment_id,
                segment_kind: row.kind.clone(),
                before_tool_call_id,
                sequence,
            });
            all_rows.push(row);
            sequence = sequence.saturating_add(1);
        }
    }
    (segments, all_rows)
}

fn segment_keys(row: &ProjectedRow, sequence: u32) -> (String, Option<String>) {
    if row.kind == TOOL_KIND {
        let id = row
            .tool_call_id
            .clone()
            .unwrap_or_else(|| format!("{TOOL_KIND}-{sequence}"));
        (id, None)
    } else if row.kind == ASSISTANT_COMMENTARY {
        let before = row.tool_call_id.clone();
        let id = before
            .as_ref()
            .map(|tid| format!("seg-before-{tid}"))
            .unwrap_or_else(|| format!("{ASSISTANT_COMMENTARY}-{sequence}"));
        (id, before)
    } else {
        (format!("{}-{sequence}", row.kind), None)
    }
}

fn rows_for_turn_slice(slice: &[Message]) -> Vec<ProjectedRow> {
    let mut turn = Turn::default();
    let reducer = TurnReducer;
    let mut answers: Vec<(u32, String)> = Vec::new();
    let mut saw_tools = false;
    for m in slice {
        if is_chat_timeline_marker(m) {
            apply_timeline(&reducer, &mut turn, m);
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
            }
            push_answer_checkpoint(&mut answers, &turn, m);
        } else {
            apply_tool_batch(&reducer, &mut turn, m, tools);
            saw_tools = true;
        }
    }
    if saw_tools {
        reducer.apply(&mut turn, TurnEvent::ToolPhaseEnd);
    }
    crate::cm_turn_layout::close_open_commentary_segments(&mut turn);
    merge_v2_with_answers(project_turn_web_v2(&turn), &answers)
}

fn push_answer_checkpoint(answers: &mut Vec<(u32, String)>, turn: &Turn, m: &Message) {
    let text = message_content_plain_for_chat_display(&m.content);
    if text.trim().is_empty() {
        return;
    }
    let after_tools = u32::try_from(turn.steps.len()).unwrap_or(u32::MAX);
    answers.push((after_tools, text));
}

fn merge_v2_with_answers(v2: Vec<ProjectedRow>, answers: &[(u32, String)]) -> Vec<ProjectedRow> {
    let mut out = Vec::with_capacity(v2.len().saturating_add(answers.len()));
    let mut ai = 0usize;
    let mut i = 0usize;
    while i < v2.len() && v2[i].kind == "assistant_timeline" {
        out.push(v2[i].clone());
        i = i.saturating_add(1);
    }
    drain_answers_at(&mut out, answers, &mut ai, 0);
    let mut tools_emitted = 0u32;
    while i < v2.len() {
        let is_tool = v2[i].kind == TOOL_KIND;
        out.push(v2[i].clone());
        i = i.saturating_add(1);
        if is_tool {
            tools_emitted = tools_emitted.saturating_add(1);
            drain_answers_at(&mut out, answers, &mut ai, tools_emitted);
        }
    }
    while ai < answers.len() {
        push_answer_text(&mut out, answers[ai].1.clone());
        ai = ai.saturating_add(1);
    }
    out
}

fn drain_answers_at(
    out: &mut Vec<ProjectedRow>,
    answers: &[(u32, String)],
    ai: &mut usize,
    after_tools: u32,
) {
    while *ai < answers.len() && answers[*ai].0 == after_tools {
        push_answer_text(out, answers[*ai].1.clone());
        *ai = ai.saturating_add(1);
    }
}

fn push_answer_text(out: &mut Vec<ProjectedRow>, text: String) {
    out.push(ProjectedRow {
        kind: ASSISTANT_ANSWER.into(),
        text,
        tool_name: None,
        tool_call_id: None,
    });
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

fn iter_turn_slices(messages: &[Message]) -> Vec<(String, &[Message])> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < messages.len() && !user_message_counts_for_branch_truncation(&messages[i]) {
        i = i.saturating_add(1);
    }
    if i > 0 {
        out.push((LEAD_TURN_ID.to_string(), &messages[..i]));
    }
    let mut ordinal = 0u32;
    while i < messages.len() {
        let start = i.saturating_add(1);
        i = start;
        while i < messages.len() && !user_message_counts_for_branch_truncation(&messages[i]) {
            i = i.saturating_add(1);
        }
        out.push((format!("u{ordinal}"), &messages[start..i]));
        ordinal = ordinal.saturating_add(1);
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
    use std::collections::HashSet;

    fn user(text: &str) -> Message {
        Message::user_only(text.to_string())
    }

    fn assistant_text(text: &str) -> Message {
        Message::assistant_only(text.to_string())
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

    fn timeline(text: &str) -> Message {
        Message {
            role: "system".into(),
            content: Some(MessageContent::Text(text.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("crabmate_timeline".into()),
            tool_call_id: None,
        }
    }

    fn kinds(meta: &crate::cm_api_contract::chat::ConversationLayoutMeta) -> Vec<&str> {
        meta.segments
            .iter()
            .map(|s| s.segment_kind.as_str())
            .collect()
    }

    #[test]
    fn empty_messages_still_versioned() {
        let meta = layout_meta_from_messages(&[]);
        assert_eq!(meta.layout_schema_version, 2);
        assert!(meta.segments.is_empty());
        assert!(meta.projection_hash.is_some());
    }

    #[test]
    fn tool_then_answer_uses_distinct_segment_keys() {
        let msgs = vec![
            user("go"),
            assistant_tools("先读。", "tc1", "read_file"),
            assistant_text("完毕。"),
        ];
        let meta = layout_meta_from_messages(&msgs);
        assert_eq!(
            kinds(&meta),
            vec!["assistant_commentary", "tool", "assistant_answer"]
        );
        assert_eq!(meta.segments[0].turn_id.as_deref(), Some("u0"));
        assert_eq!(meta.segments[0].segment_id, "seg-before-tc1");
        assert_eq!(meta.segments[0].before_tool_call_id.as_deref(), Some("tc1"));
        assert_eq!(meta.segments[1].segment_id, "tc1");
        assert!(meta.segments[1].before_tool_call_id.is_none());
        let ids: HashSet<_> = meta
            .segments
            .iter()
            .map(|s| s.segment_id.as_str())
            .collect();
        assert_eq!(ids.len(), meta.segments.len());
    }

    #[test]
    fn interleaved_answer_stays_between_tool_batches() {
        let msgs = vec![
            user("go"),
            assistant_tools("一。", "tc1", "read_file"),
            assistant_text("中段。"),
            assistant_tools("二。", "tc2", "list_tree"),
        ];
        assert_eq!(
            kinds(&layout_meta_from_messages(&msgs)),
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
    fn timeline_after_tools_still_projects_first() {
        let msgs = vec![
            user("go"),
            assistant_tools("读。", "tc1", "read_file"),
            timeline("审批通过"),
            assistant_text("好。"),
        ];
        let meta = layout_meta_from_messages(&msgs);
        assert_eq!(
            kinds(&meta),
            vec![
                "assistant_timeline",
                "assistant_commentary",
                "tool",
                "assistant_answer"
            ]
        );
        assert_eq!(meta.segments[0].segment_id, "assistant_timeline-0");
        assert!(meta.segments[2].before_tool_call_id.is_none());
        assert_eq!(meta.segments[2].segment_id, "tc1");
    }

    #[test]
    fn timeline_before_first_user_uses_lead_turn() {
        let msgs = vec![timeline("开场"), user("hi"), assistant_text("ok")];
        let meta = layout_meta_from_messages(&msgs);
        assert_eq!(kinds(&meta), vec!["assistant_timeline", "assistant_answer"]);
        assert_eq!(meta.segments[0].turn_id.as_deref(), Some("lead"));
        assert_eq!(meta.segments[1].turn_id.as_deref(), Some("u0"));
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
