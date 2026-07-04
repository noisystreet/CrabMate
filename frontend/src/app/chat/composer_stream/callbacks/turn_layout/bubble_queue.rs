//! 块布局：流式 delta → loading overlay preview；`StoredMessage` upsert 对齐 [`project_turn_web`]。

use crabmate_turn_layout::{
    ASSISTANT_BATCH_NARRATION, project_turn_web, streaming_commentary_block_text,
};

use super::super::super::turn_canonical::TurnCanonicalState;

/// 单轮工具批前说明块的稳定 id（与 `project_turn_web` · `assistant_batch_narration` 对应）。
pub(super) const BATCH_NARRATION_ROW_ID: &str = "turn-batch-narration";

const PROJECT_KIND_BATCH_NARRATION: &str = ASSISTANT_BATCH_NARRATION;

/// 流式 preview / 边界 flush 队列。
#[derive(Default, Debug)]
pub(crate) struct BubbleOutputQueue;

impl BubbleOutputQueue {
    fn batch_row_from_projection(
        turn: &TurnCanonicalState,
    ) -> Option<crabmate_turn_layout::ProjectedRow> {
        project_turn_web(turn.turn_ref())
            .into_iter()
            .find(|r| r.kind == PROJECT_KIND_BATCH_NARRATION)
    }

    fn insert_index_for_batch_row(
        messages: &[crate::storage::StoredMessage],
        anchor_tool_call_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(tcid) = anchor_tool_call_id.filter(|t| !t.is_empty()) {
            if let Some(idx) = messages
                .iter()
                .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tcid))
            {
                return Some(idx);
            }
        }
        messages.iter().position(|m| m.is_tool)
    }

    /// loading 尾泡 overlay：**仅**未落盘的增量（open commentary 段或 post-tool 终答）。
    pub(super) fn loading_preview_text(turn: &TurnCanonicalState) -> String {
        if turn.tool_phase_open() {
            return streaming_commentary_block_text(turn.turn_ref()).unwrap_or_default();
        }
        turn.turn_ref().final_answer.clone().unwrap_or_default()
    }

    /// 按 [`project_turn_web`] upsert `turn-batch-narration` 行。
    pub(super) fn flush_batch_narration_row(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
    ) {
        if turn.pending_stream_commentary_open()
            && !messages.iter().any(|m| m.id == BATCH_NARRATION_ROW_ID)
        {
            return;
        }
        let Some(batch) = Self::batch_row_from_projection(turn) else {
            return;
        };
        if batch.text.trim().is_empty() {
            return;
        }
        let Some(insert_idx) =
            Self::insert_index_for_batch_row(messages, batch.tool_call_id.as_deref())
        else {
            return;
        };
        if let Some(idx) = messages.iter().position(|m| m.id == BATCH_NARRATION_ROW_ID) {
            if messages[idx].text != batch.text {
                messages[idx].text = batch.text.clone();
            }
            if messages[idx].tool_call_id.is_some() {
                messages[idx].tool_call_id = None;
            }
            if idx != insert_idx {
                let row = messages.remove(idx);
                let insert_idx =
                    Self::insert_index_for_batch_row(messages, batch.tool_call_id.as_deref())
                        .unwrap_or(insert_idx);
                messages.insert(insert_idx, row);
            }
            return;
        }
        let row = crate::storage::StoredMessage {
            id: BATCH_NARRATION_ROW_ID.to_string(),
            role: "assistant".to_string(),
            text: batch.text.clone(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: {
                #[cfg(target_arch = "wasm32")]
                {
                    crate::session_ops::message_created_ms()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    0
                }
            },
        };
        messages.insert(insert_idx, row);
    }

    /// preview 是否应写入 loading 尾泡（与 stored 一致则不再 duplicate）。
    pub(super) fn loading_preview_for_messages(
        turn: &TurnCanonicalState,
        messages: &[crate::storage::StoredMessage],
    ) -> String {
        let preview = Self::loading_preview_text(turn);
        if preview.trim().is_empty() {
            return String::new();
        }
        if !turn.tool_phase_open()
            && let Some(load) = messages.iter().find(|m| {
                m.role == "assistant"
                    && !m.is_tool
                    && m.state.as_ref().is_some_and(|st| st.is_loading())
            })
            && load.text.trim() == preview.trim()
        {
            return String::new();
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse_dispatch::TurnSegmentStartInfo;

    fn make_turn_with_batch_commentary() -> TurnCanonicalState {
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
    fn loading_preview_during_tool_phase_is_open_segment_only() {
        let mut turn = TurnCanonicalState::new();
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_a".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 A。"));
        turn.on_segment_end("seg-before-tc_a".into());
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_b".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_b".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 B。"));
        assert_eq!(
            crabmate_turn_layout::batch_narration_text(turn.turn_ref()).as_deref(),
            Some("步骤 A。")
        );
        assert_eq!(
            BubbleOutputQueue::loading_preview_text(&turn).as_str(),
            "步骤 B。"
        );
    }

    #[test]
    fn flush_batch_narration_inserts_single_row_before_first_tool() {
        let turn = make_turn_with_batch_commentary();
        let queue = BubbleOutputQueue;
        let mut msgs = vec![crate::storage::StoredMessage {
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
        }];
        queue.flush_batch_narration_row(&mut msgs, &turn);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, BATCH_NARRATION_ROW_ID);
        assert_eq!(msgs[0].text, "步骤 A。");
        assert_eq!(msgs[1].id, "t");
        queue.flush_batch_narration_row(&mut msgs, &turn);
        assert_eq!(msgs.len(), 2, "second flush must not duplicate row");
    }

    #[test]
    fn flush_batch_narration_skips_without_tool_row() {
        let turn = make_turn_with_batch_commentary();
        let queue = BubbleOutputQueue;
        let mut msgs = Vec::new();
        queue.flush_batch_narration_row(&mut msgs, &turn);
        assert!(msgs.is_empty());
    }
}
