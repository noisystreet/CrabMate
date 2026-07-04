//! 块布局：流式 delta → loading overlay preview；`StoredMessage` upsert 对齐 [`project_turn_web`]。

use crabmate_turn_layout::{
    ASSISTANT_BATCH_NARRATION, project_turn_web, streaming_commentary_block_text,
};

use super::super::super::turn_canonical::TurnCanonicalState;

/// 单轮工具批前说明块的稳定 id（与 `project_turn_web` · `assistant_batch_narration` 对应）。
pub(crate) const BATCH_NARRATION_ROW_ID: &str = "turn-batch-narration";
/// 工具批结束后终答块的稳定 id（与 `project_turn_web` · `assistant_answer` 对应）。
pub(crate) const FINAL_ANSWER_ROW_ID: &str = "turn-final-answer";

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

    fn insert_index_before_loading_tail(
        messages: &[crate::storage::StoredMessage],
        loading_tail_id: Option<&str>,
    ) -> usize {
        if let Some(id) = loading_tail_id.filter(|t| !t.is_empty()) {
            if let Some(idx) = messages.iter().position(|m| m.id == id) {
                return idx;
            }
        }
        messages.len()
    }

    fn upsert_assistant_row(
        messages: &mut Vec<crate::storage::StoredMessage>,
        row_id: &str,
        text: String,
        insert_idx: usize,
    ) {
        if text.trim().is_empty() {
            return;
        }
        if let Some(idx) = messages.iter().position(|m| m.id == row_id) {
            if messages[idx].text != text {
                messages[idx].text = text.clone();
            }
            if messages[idx].tool_call_id.is_some() {
                messages[idx].tool_call_id = None;
            }
            if idx != insert_idx {
                let row = messages.remove(idx);
                let insert_idx = insert_idx.min(messages.len());
                messages.insert(insert_idx, row);
            }
            return;
        }
        let row = crate::storage::StoredMessage {
            id: row_id.to_string(),
            role: "assistant".to_string(),
            text,
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
        messages.insert(insert_idx.min(messages.len()), row);
    }

    /// Phase 9：**唯一** Web assistant 正文落盘入口（batch + final + 清空 loading 壳正文）。
    pub(super) fn sync_web_projection(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
    ) {
        self.flush_batch_narration_row(messages, turn);
        self.flush_final_answer_row(messages, turn, loading_tail_id);
        Self::clear_loading_tail_stored_answer_body(messages, loading_tail_id);
    }

    /// loading 壳 **不得** 持有与投影行重复的 assistant 正文（真源 = batch/final 行 + overlay preview）。
    fn clear_loading_tail_stored_answer_body(
        messages: &mut [crate::storage::StoredMessage],
        loading_tail_id: Option<&str>,
    ) {
        let Some(id) = loading_tail_id.filter(|t| !t.is_empty()) else {
            return;
        };
        let Some(idx) = messages.iter().position(|m| m.id == id) else {
            return;
        };
        if messages[idx].role != "assistant" || messages[idx].is_tool {
            return;
        }
        messages[idx].text.clear();
    }

    /// 按 [`project_turn_web`] upsert `turn-batch-narration` 行。
    pub(super) fn flush_batch_narration_row(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
    ) {
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
        Self::upsert_assistant_row(
            messages,
            BATCH_NARRATION_ROW_ID,
            batch.text.clone(),
            insert_idx,
        );
    }

    /// 工具批结束后 upsert `turn-final-answer`（位于 loading 尾泡之前）。
    pub(super) fn flush_final_answer_row(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
    ) {
        if turn.tool_phase_open() {
            return;
        }
        let Some(text) = turn
            .turn_ref()
            .final_answer
            .as_ref()
            .filter(|t| !t.trim().is_empty())
            .cloned()
        else {
            return;
        };
        let insert_idx = Self::insert_index_before_loading_tail(messages, loading_tail_id);
        Self::upsert_assistant_row(messages, FINAL_ANSWER_ROW_ID, text, insert_idx);
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
        if !turn.tool_phase_open() {
            if let Some(final_row) = messages.iter().find(|m| m.id == FINAL_ANSWER_ROW_ID) {
                if final_row.text.trim() == preview.trim() {
                    return String::new();
                }
            }
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
    fn sync_web_projection_clears_loading_stored_body() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_answer_delta("完成。"));
        turn.on_tool_phase_end();
        let queue = BubbleOutputQueue;
        let mut msgs = vec![
            crate::storage::StoredMessage {
                id: BATCH_NARRATION_ROW_ID.into(),
                role: "assistant".into(),
                text: "说明。".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            crate::storage::StoredMessage {
                id: "load".into(),
                role: "assistant".into(),
                text: "不应落盘的尾泡正文".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(crate::storage::StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        queue.sync_web_projection(&mut msgs, &turn, Some("load"));
        let load = msgs.iter().find(|m| m.id == "load").expect("loading shell");
        assert_eq!(load.text, "");
        assert!(
            msgs.iter()
                .any(|m| m.id == FINAL_ANSWER_ROW_ID && m.text == "完成。")
        );
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
