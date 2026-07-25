//! v2 布局：流式 delta → loading overlay preview；已关闭 commentary 按工具键不可变落盘。

use crabmate_turn_layout::{
    ASSISTANT_COMMENTARY, project_turn_web_v2, streaming_commentary_block_text,
};

use crate::message_loading::is_loading_plain_assistant;
use crate::storage::{V2_COMMENTARY_ROW_ID_PREFIX, V2_FINAL_ANSWER_ROW_ID};

use super::super::super::turn_canonical::TurnCanonicalState;

/// 工具批结束后终答块的稳定 id（与 `project_turn_web` · `assistant_answer` 对应）。
pub(crate) const FINAL_ANSWER_ROW_ID: &str = V2_FINAL_ANSWER_ROW_ID;

const PROJECT_KIND_COMMENTARY: &str = ASSISTANT_COMMENTARY;

pub(crate) fn commentary_row_id(tool_call_id: &str) -> String {
    format!("{V2_COMMENTARY_ROW_ID_PREFIX}{tool_call_id}")
}

pub(crate) fn is_commentary_row_id(message_id: &str) -> bool {
    message_id.starts_with(V2_COMMENTARY_ROW_ID_PREFIX)
}

/// 流式 preview / 边界 flush 队列。
#[derive(Default, Debug)]
pub(crate) struct BubbleOutputQueue;

impl BubbleOutputQueue {
    fn commentary_rows_from_projection(
        turn: &TurnCanonicalState,
    ) -> Vec<crabmate_turn_layout::ProjectedRow> {
        project_turn_web_v2(turn.turn_ref())
            .into_iter()
            .filter(|row| row.kind == PROJECT_KIND_COMMENTARY)
            .collect()
    }

    fn insert_index_for_commentary_row(
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
    pub(super) fn loading_preview_text(
        turn: &TurnCanonicalState,
        overlay_answer: Option<&str>,
    ) -> String {
        if turn.tool_phase_open() {
            return streaming_commentary_block_text(turn.turn_ref()).unwrap_or_default();
        }
        overlay_answer
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_default()
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
                let mut at = insert_idx;
                if idx < at {
                    at -= 1;
                }
                messages.insert(at.min(messages.len()), row);
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

    fn insert_finalized_assistant_row(
        messages: &mut Vec<crate::storage::StoredMessage>,
        row_id: &str,
        text: String,
        insert_idx: usize,
    ) {
        if text.trim().is_empty() || messages.iter().any(|message| message.id == row_id) {
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

    /// Web assistant 正文落盘入口（不可变 commentary + final）。
    ///
    /// `overlay_answer`：当前 loading 尾泡的 overlay 正文（终答唯一来源）。
    pub(super) fn sync_web_projection(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
        overlay_answer: Option<&str>,
    ) {
        self.flush_commentary_rows(messages, turn, overlay_answer);
        self.flush_final_answer_row(messages, turn, loading_tail_id, overlay_answer);
    }

    fn commentary_projection_pending_in_messages(
        messages: &[crate::storage::StoredMessage],
        turn: &TurnCanonicalState,
    ) -> bool {
        Self::commentary_rows_from_projection(turn)
            .into_iter()
            .filter_map(|row| row.tool_call_id)
            .map(|tool_call_id| commentary_row_id(tool_call_id.as_str()))
            .any(|row_id| messages.iter().all(|message| message.id != row_id))
    }

    fn insert_index_for_final_row(
        messages: &[crate::storage::StoredMessage],
        loading_tail_id: Option<&str>,
    ) -> usize {
        let mut insert_idx = Self::insert_index_before_loading_tail(messages, loading_tail_id);
        if let Some(commentary_idx) = messages
            .iter()
            .enumerate()
            .filter(|(_, message)| is_commentary_row_id(message.id.as_str()))
            .map(|(idx, _)| idx)
            .max()
        {
            insert_idx = insert_idx.max(commentary_idx + 1);
        }
        insert_idx
    }

    /// 发布按工具调用键控的不可变 commentary 行。
    pub(super) fn flush_commentary_rows(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        _overlay_answer: Option<&str>,
    ) {
        for commentary in Self::commentary_rows_from_projection(turn) {
            let Some(tool_call_id) = commentary.tool_call_id.as_deref() else {
                continue;
            };
            let Some(insert_idx) =
                Self::insert_index_for_commentary_row(messages, Some(tool_call_id))
            else {
                continue;
            };
            Self::insert_finalized_assistant_row(
                messages,
                commentary_row_id(tool_call_id).as_str(),
                commentary.text,
                insert_idx,
            );
        }
    }

    /// 工具批结束后 upsert `turn-final-answer`（位于 loading 尾泡之前）。
    ///
    /// 从 overlay 读取终答正文。
    pub(super) fn flush_final_answer_row(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
        overlay_answer: Option<&str>,
    ) {
        if turn.tool_phase_open() {
            return;
        }
        if Self::commentary_projection_pending_in_messages(messages, turn) {
            return;
        }
        let text = overlay_answer
            .filter(|t| !t.trim().is_empty())
            .map(str::to_string);
        let Some(text) = text else {
            return;
        };
        // 若已有普通 assistant 行内容相同（由 detach_final_answer_projection 产生），
        // 不再重复创建 FINAL_ANSWER_ROW，避免消息双倍
        if messages.iter().any(|m| {
            m.id != FINAL_ANSWER_ROW_ID
                && m.role == "assistant"
                && !m.is_tool
                && m.state.is_none()
                && m.text.trim() == text.trim()
        }) {
            return;
        }
        let insert_idx = Self::insert_index_for_final_row(messages, loading_tail_id);
        Self::upsert_assistant_row(messages, FINAL_ANSWER_ROW_ID, text, insert_idx);
    }

    /// 若 FINAL_ANSWER_ROW 缺失，从给定正文补建。
    ///
    /// 零工具场景中 overlay 可能在 `sync_turn_projection` 前已被清空
    /// （流式 delta 写入 loading 尾泡而非 overlay），导致 `flush_final_answer_row` 读不到
    /// overlay。此时 `drain` 将 loading 正文合并到 stored 后，调用此方法补建
    /// FINAL_ANSWER_ROW 以持久化终答。
    pub(super) fn ensure_final_answer_row_from_text(
        messages: &mut Vec<crate::storage::StoredMessage>,
        text: &str,
        loading_tail_id: Option<&str>,
    ) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if messages
            .iter()
            .any(|m| m.id == FINAL_ANSWER_ROW_ID && !m.text.trim().is_empty())
        {
            return;
        }
        let insert_idx = Self::insert_index_for_final_row(messages, loading_tail_id);
        Self::upsert_assistant_row(
            messages,
            FINAL_ANSWER_ROW_ID,
            trimmed.to_string(),
            insert_idx,
        );
    }

    /// preview 是否应写入 loading 尾泡（与 stored 一致则不再 duplicate）。
    pub(super) fn loading_preview_for_messages(
        turn: &TurnCanonicalState,
        messages: &[crate::storage::StoredMessage],
        overlay_answer: Option<&str>,
    ) -> String {
        let preview = Self::loading_preview_text(turn, overlay_answer);
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
            && let Some(load) = messages.iter().find(|m| is_loading_plain_assistant(m))
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

    fn make_turn_with_commentary() -> TurnCanonicalState {
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
            crabmate_turn_layout::commentary_for_tool(turn.turn_ref(), "tc_a").as_deref(),
            Some("步骤 A。")
        );
        assert_eq!(
            BubbleOutputQueue::loading_preview_text(&turn, None).as_str(),
            "步骤 B。"
        );
    }

    #[test]
    fn flush_commentary_inserts_immutable_row_before_its_tool() {
        let turn = make_turn_with_commentary();
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
        queue.flush_commentary_rows(&mut msgs, &turn, None);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].id, commentary_row_id("tc_a"));
        assert_eq!(msgs[0].text, "步骤 A。");
        assert_eq!(msgs[1].id, "t");
        queue.flush_commentary_rows(&mut msgs, &turn, None);
        assert_eq!(msgs.len(), 2, "second flush must not duplicate row");
    }

    #[test]
    fn sync_web_projection_keeps_loading_body() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_answer_state_transition("完成。"));
        turn.on_tool_phase_end();
        let queue = BubbleOutputQueue;
        let mut msgs = vec![
            crate::storage::StoredMessage {
                id: commentary_row_id("tc_existing"),
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
        // 终答在 overlay；模拟 overlay 已有终答。
        queue.sync_web_projection(&mut msgs, &turn, Some("load"), Some("完成。"));
        // loading tail 保留正文（不再清空，避免聊天列气泡闪烁）
        let load = msgs.iter().find(|m| m.id == "load").expect("loading shell");
        assert_eq!(load.text, "不应落盘的尾泡正文");
        assert!(
            msgs.iter()
                .any(|m| m.id == FINAL_ANSWER_ROW_ID && m.text == "完成。")
        );
    }

    #[test]
    fn flush_commentary_does_not_move_existing_finalized_row() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_archive", "archive_list", "list");
        turn.on_segment_start(crate::sse_dispatch::TurnSegmentStartInfo {
            segment_id: "seg-before-tc_unpack".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_unpack".into()),
        });
        assert!(turn.try_apply_commentary_delta("好的，先解压。"));
        turn.on_tool_call("tc_unpack", "unpack", "unpack");

        let queue = BubbleOutputQueue;
        let mut msgs = vec![
            crate::storage::StoredMessage {
                id: "tc_archive".into(),
                role: "system".into(),
                text: "archive".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: Some("tc_archive".into()),
                tool_name: None,
                created_at: 0,
            },
            crate::storage::StoredMessage {
                id: "tc_unpack".into(),
                role: "system".into(),
                text: "unpack".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: Some("tc_unpack".into()),
                tool_name: None,
                created_at: 0,
            },
            crate::storage::StoredMessage {
                id: commentary_row_id("tc_unpack"),
                role: "assistant".into(),
                text: "好的，先解压。".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        queue.flush_commentary_rows(&mut msgs, &turn, None);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].id, "tc_archive");
        assert_eq!(msgs[1].id, "tc_unpack");
        assert_eq!(msgs[2].id, commentary_row_id("tc_unpack"));
    }

    #[test]
    fn flush_final_deferred_until_commentary_row_present() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        turn.on_segment_start(crate::sse_dispatch::TurnSegmentStartInfo {
            segment_id: "seg-before-tc_a".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("批说明。"));
        turn.on_segment_end("seg-before-tc_a".into());
        turn.on_tool_phase_end();
        assert!(turn.try_apply_answer_state_transition("终答。"));

        let queue = BubbleOutputQueue;
        let mut msgs = vec![crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }];
        // 终答在 overlay；模拟 overlay 已有终答。
        queue.flush_final_answer_row(&mut msgs, &turn, Some("load"), Some("终答。"));
        assert!(
            !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
            "final must not appear before commentary row"
        );

        msgs.insert(
            0,
            crate::storage::StoredMessage {
                id: "tc_a".into(),
                role: "system".into(),
                text: "tool".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                tool_call_id: Some("tc_a".into()),
                tool_name: None,
                created_at: 0,
            },
        );
        queue.sync_web_projection(&mut msgs, &turn, Some("load"), Some("终答。"));
        let commentary_idx = msgs
            .iter()
            .position(|m| m.id == commentary_row_id("tc_a"))
            .expect("commentary");
        let final_idx = msgs
            .iter()
            .position(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("final");
        assert!(
            commentary_idx < final_idx,
            "commentary must precede final in stored order"
        );
    }

    #[test]
    fn flush_commentary_skips_without_tool_row() {
        let turn = make_turn_with_commentary();
        let queue = BubbleOutputQueue;
        let mut msgs = Vec::new();
        queue.flush_commentary_rows(&mut msgs, &turn, None);
        assert!(msgs.is_empty());
    }

    /// 无工具场景：`flush_final_answer_row` 从 overlay 创建 FINAL_ANSWER_ROW。
    ///
    /// 这是无工具问答的正常路径：流式 delta 写入 overlay，on_done 时
    /// `flush_final_answer_row` 读 overlay 创建终答行。
    #[test]
    fn no_tool_flush_final_creates_row_from_overlay() {
        let turn = TurnCanonicalState::new();
        let queue = BubbleOutputQueue;
        let mut msgs = vec![crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }];
        queue.sync_web_projection(&mut msgs, &turn, Some("load"), Some("无工具终答正文。"));
        // FINAL_ANSWER_ROW 应创建
        let final_row = msgs
            .iter()
            .find(|m| m.id == FINAL_ANSWER_ROW_ID)
            .expect("FINAL_ANSWER_ROW must be created in no-tool scenario");
        assert_eq!(final_row.text, "无工具终答正文。");
        // loading tail 仍保留
        assert!(msgs.iter().any(|m| m.id == "load"));
    }

    /// 无工具场景 overlay 为空时：`flush_final_answer_row` 不应创建
    /// FINAL_ANSWER_ROW（对应 overlay 被 prematurely 清空的情况）。
    #[test]
    fn no_tool_flush_final_skips_when_overlay_empty() {
        let turn = TurnCanonicalState::new();
        let queue = BubbleOutputQueue;
        let mut msgs = vec![crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }];
        queue.sync_web_projection(&mut msgs, &turn, Some("load"), None);
        // overlay 为空时不应创建 FINAL_ANSWER_ROW
        assert!(
            !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
            "FINAL_ANSWER_ROW must not be created when overlay is empty"
        );
    }
}
