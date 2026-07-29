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

    /// 将旁注 upsert 到锚定工具行之前（可更新正文；若误落在工具后则搬回）。
    ///
    /// 用于：已关闭旁注 flush，以及晚到 open 旁注在工具行已存在时的流式预览。
    /// 返回是否已把正文挂在工具前的 commentary 行上。
    pub(super) fn upsert_commentary_before_tool(
        messages: &mut Vec<crate::storage::StoredMessage>,
        tool_call_id: &str,
        text: String,
    ) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(tool_idx) = messages
            .iter()
            .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tool_call_id))
        else {
            return false;
        };
        let row_id = commentary_row_id(tool_call_id);
        if let Some(idx) = messages.iter().position(|m| m.id == row_id) {
            if messages[idx].text != text {
                messages[idx].text = text;
            }
            if idx > tool_idx {
                let row = messages.remove(idx);
                let new_tool_idx = messages
                    .iter()
                    .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tool_call_id))
                    .unwrap_or(tool_idx);
                messages.insert(new_tool_idx, row);
            }
            return true;
        }
        let row = Self::new_commentary_row(row_id, text);
        messages.insert(tool_idx, row);
        true
    }

    fn new_commentary_row(row_id: String, text: String) -> crate::storage::StoredMessage {
        crate::storage::StoredMessage {
            id: row_id,
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
        }
    }

    /// Phase B：open 锚定旁白直接写 `turn-commentary-*`（工具尚不存在则暂挂在 loading 前）。
    ///
    /// 工具到达后由 [`Self::upsert_commentary_before_tool`] / flush 搬到工具前。
    pub(super) fn upsert_streaming_anchored_commentary(
        messages: &mut Vec<crate::storage::StoredMessage>,
        tool_call_id: &str,
        text: String,
        loading_tail_id: Option<&str>,
    ) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        if messages
            .iter()
            .any(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tool_call_id))
        {
            return Self::upsert_commentary_before_tool(messages, tool_call_id, text);
        }
        let row_id = commentary_row_id(tool_call_id);
        let insert_idx = Self::insert_index_before_loading_tail(messages, loading_tail_id);
        if let Some(idx) = messages.iter().position(|m| m.id == row_id) {
            if messages[idx].text != text {
                messages[idx].text = text;
            }
            if let Some(load_id) = loading_tail_id.filter(|t| !t.is_empty())
                && let Some(load_idx) = messages.iter().position(|m| m.id == load_id)
                && idx > load_idx
            {
                let row = messages.remove(idx);
                let new_load = messages
                    .iter()
                    .position(|m| m.id == load_id)
                    .unwrap_or(messages.len());
                messages.insert(new_load, row);
            }
            return true;
        }
        messages.insert(
            insert_idx.min(messages.len()),
            Self::new_commentary_row(row_id, text),
        );
        true
    }

    /// loading 尾泡 overlay：**仅**未落盘终答（或无锚点的短暂 open 段）。
    ///
    /// 带 `before_tool_call_id` 的 open commentary 一律由
    /// [`Self::upsert_streaming_anchored_commentary`] 承载，此处返回空。
    pub(super) fn loading_preview_text(
        turn: &TurnCanonicalState,
        overlay_answer: Option<&str>,
        _messages: Option<&[crate::storage::StoredMessage]>,
    ) -> String {
        if turn.tool_phase_open() {
            if crabmate_turn_layout::streaming_commentary_before_tool(turn.turn_ref()).is_some() {
                return String::new();
            }
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

    // dead: kept as insert-once helper for potential no-upsert paths; prefer upsert_*.
    #[allow(dead_code)]
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
    /// `allow_final_answer`：为 false 时不写 `turn-final-answer`（工具前旁白仍在 overlay/loading，
    /// 避免 `turn_segment_end` 把旁白误刷进终答行，随后 demote/detach 造成助手气泡闪消失）。
    pub(super) fn sync_web_projection(
        &self,
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
        overlay_answer: Option<&str>,
        allow_final_answer: bool,
    ) {
        self.flush_commentary_rows(messages, turn, overlay_answer);
        if allow_final_answer {
            self.flush_final_answer_row(messages, turn, loading_tail_id, overlay_answer);
        }
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

    /// 发布按工具调用键控的 commentary 行（可更新正文；始终位于锚定工具之前）。
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
            let _ = Self::upsert_commentary_before_tool(messages, tool_call_id, commentary.text);
        }
    }

    /// 工具相 open 锚定旁白：直接 upsert `turn-commentary-*`（工具可尚未存在）。
    pub(super) fn try_upsert_open_anchored_commentary(
        messages: &mut Vec<crate::storage::StoredMessage>,
        turn: &TurnCanonicalState,
        loading_tail_id: Option<&str>,
    ) -> bool {
        if !turn.tool_phase_open() {
            return false;
        }
        let Some((tcid, text)) =
            crabmate_turn_layout::streaming_commentary_before_tool(turn.turn_ref())
        else {
            return false;
        };
        Self::upsert_streaming_anchored_commentary(messages, tcid.as_str(), text, loading_tail_id)
    }

    /// 工具批结束后 upsert `turn-final-answer`（位于 loading 尾泡之前）。
    ///
    /// 从 overlay 读取终答正文。调用方须已确认允许落盘终答（post-tool / on_done）。
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
        let preview = Self::loading_preview_text(turn, overlay_answer, Some(messages));
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
#[path = "bubble_queue_tests.rs"]
mod tests;
