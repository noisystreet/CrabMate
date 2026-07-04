//! 完整气泡输出队列：流式 delta 只更新 preview（loading 尾泡）；段/工具边界再将 closed 块落盘为独立行。
//!
//! 与 canonical [`TurnCanonicalState`] 分工：reducer 归并真值；本队列控制 **何时** 向 `messages` 插入完整 assistant 行，
//! 避免每个 SSE 片段触发列表重排与 `<For>` reconciliation 闪烁。

use std::collections::HashSet;

use crabmate_turn_layout::{SegmentKind, Turn, commentary_for_tool};

use super::super::super::turn_canonical::TurnCanonicalState;

use super::{
    find_commentary_before_tool_index, find_tool_index_latest,
    sync_commentary_before_tool_in_messages,
};

/// 单轮流式：已成功落盘为独立 assistant 行的工具前旁注（按 `tool_call_id` 去重）。
#[derive(Default, Debug)]
pub(crate) struct BubbleOutputQueue {
    emitted_commentary: HashSet<String>,
}

fn commentary_tool_anchors(turn: &Turn) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for seg in &turn.segments {
        if seg.kind == SegmentKind::Commentary
            && !seg.open
            && let Some(tcid) = seg.before_tool_call_id.as_ref()
            && !tcid.is_empty()
            && seen.insert(tcid.clone())
        {
            out.push(tcid.clone());
        }
    }
    for step in &turn.steps {
        if seen.insert(step.tool_call_id.clone()) {
            out.push(step.tool_call_id.clone());
        }
    }
    out
}

impl BubbleOutputQueue {
    /// loading 尾泡应显示的 **未落盘** 流式 preview（open 段或终答增量）。
    pub(super) fn loading_preview_text(turn: &TurnCanonicalState) -> String {
        if turn.tool_phase_open() {
            return turn.streaming_commentary_block_text().unwrap_or_default();
        }
        turn.turn_ref().final_answer.clone().unwrap_or_default()
    }

    /// 将 canonical 中 **已关闭** 的工具前旁注块写入 `messages`（每工具至多 insert 一次；已 emit 仍允许 replace 文本）。
    pub(super) fn flush_complete_commentary_rows(
        &mut self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
    ) {
        if turn.pending_stream_commentary_open() {
            return;
        }
        for tcid in commentary_tool_anchors(turn.turn_ref()) {
            if turn.has_open_commentary_segment_for_tool(tcid.as_str()) {
                continue;
            }
            let Some(text) = commentary_for_tool(turn.turn_ref(), tcid.as_str()) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            if self.emitted_commentary.contains(tcid.as_str()) {
                sync_commentary_before_tool_in_messages(messages, tcid.as_str(), text.as_str());
                continue;
            }
            if sync_commentary_before_tool_in_messages(messages, tcid.as_str(), text.as_str()) {
                self.emitted_commentary.insert(tcid);
            }
        }
    }

    /// preview 是否应写入 loading 尾泡（已有同锚点旁注行则不再 duplicate 显示）。
    pub(super) fn loading_preview_for_messages(
        turn: &TurnCanonicalState,
        messages: &[crate::storage::StoredMessage],
    ) -> String {
        let mut block = Self::loading_preview_text(turn);
        if turn.tool_phase_open() {
            if let Some(anchor) = turn.open_commentary_stream_anchor_tool_call_id() {
                if let Some(tool_idx) = find_tool_index_latest(messages, anchor.as_str()) {
                    if find_commentary_before_tool_index(messages, anchor.as_str(), tool_idx)
                        .is_some()
                    {
                        block.clear();
                    }
                }
            }
        }
        block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse_dispatch::TurnSegmentStartInfo;

    fn make_turn_with_closed_commentary() -> TurnCanonicalState {
        let mut turn = TurnCanonicalState::new();
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_a".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 A。"));
        turn.on_segment_end("seg-before-tc_a".into());
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        turn
    }

    #[test]
    fn queue_emits_commentary_once_per_tool() {
        let turn = make_turn_with_closed_commentary();
        let mut queue = BubbleOutputQueue::default();
        let mut msgs = vec![
            crate::storage::StoredMessage {
                id: "commentary-before-tc_a".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: Some("tc_a".into()),
                tool_name: None,
                created_at: 0,
            },
            crate::storage::StoredMessage {
                id: "t".into(),
                role: "system".into(),
                text: "tool a".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: Some("tc_a".into()),
                tool_name: None,
                created_at: 0,
            },
        ];
        queue.flush_complete_commentary_rows(&mut msgs, &turn);
        assert_eq!(msgs[0].text, "步骤 A。");
        assert_eq!(msgs.len(), 2);
        queue.flush_complete_commentary_rows(&mut msgs, &turn);
        assert_eq!(msgs.len(), 2, "second flush must not duplicate row");
        assert!(queue.emitted_commentary.contains("tc_a"));
    }

    #[test]
    fn flush_before_tool_row_does_not_mark_emitted() {
        let mut turn = TurnCanonicalState::new();
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_a".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 A。"));
        turn.on_segment_end("seg-before-tc_a".into());

        let mut queue = BubbleOutputQueue::default();
        let mut msgs = Vec::new();
        queue.flush_complete_commentary_rows(&mut msgs, &turn);
        assert!(msgs.is_empty());
        assert!(
            !queue.emitted_commentary.contains("tc_a"),
            "must not mark emitted when tool row missing"
        );
    }

    #[test]
    fn flush_marks_emitted_after_tool_row_without_insert() {
        let turn = make_turn_with_closed_commentary();
        let mut queue = BubbleOutputQueue::default();
        let mut msgs = vec![
            crate::storage::StoredMessage {
                id: "commentary-before-tc_a".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: Some("tc_a".into()),
                tool_name: None,
                created_at: 0,
            },
            crate::storage::StoredMessage {
                id: "t".into(),
                role: "system".into(),
                text: "tool a".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: Some("tc_a".into()),
                tool_name: None,
                created_at: 0,
            },
        ];
        queue.flush_complete_commentary_rows(&mut msgs, &turn);
        assert_eq!(msgs[0].text, "步骤 A。");
        assert!(queue.emitted_commentary.contains("tc_a"));
    }
}
