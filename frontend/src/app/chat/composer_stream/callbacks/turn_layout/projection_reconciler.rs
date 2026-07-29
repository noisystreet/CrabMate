//! 将 [`crabmate_turn_layout::TurnProjection`] 落到 `StoredMessage`。
//!
//! Phase D：定稿旁白、锚定 active、终答 flush、工具占位插入均经本模块；
//! [`super::TurnLayout`] 只做 scratch / overlay 编排。Loading 句柄不承载旁白/终答正文。

use crabmate_turn_layout::{ASSISTANT_COMMENTARY, TurnProjection, project_turn_projection};

use crate::message_loading::stored_message_is_loading;
use crate::storage::StoredMessage;

use super::super::super::turn_canonical::TurnCanonicalState;
use super::bubble_queue::{
    BubbleOutputQueue, FINAL_ANSWER_ROW_ID, commentary_row_id, is_commentary_row_id,
};

/// 段/工具边界：定稿旁白 +（可选）终答落盘。
pub(super) fn reconcile_web_projection(
    messages: &mut Vec<StoredMessage>,
    turn: &TurnCanonicalState,
    loading_tail_id: Option<&str>,
    overlay_answer: Option<&str>,
    allow_final_answer: bool,
) {
    let projection = project_turn_projection(turn.turn_ref());
    reconcile_finalized_commentary(messages, &projection);
    if allow_final_answer {
        reconcile_final_answer_from_overlay(messages, turn, loading_tail_id, overlay_answer);
    }
}

/// 将投影中的已关闭 commentary 行 upsert 到锚定工具前。
pub(super) fn reconcile_finalized_commentary(
    messages: &mut Vec<StoredMessage>,
    projection: &TurnProjection,
) {
    for row in &projection.finalized_rows {
        if row.kind != ASSISTANT_COMMENTARY {
            continue;
        }
        let Some(tool_call_id) = row.tool_call_id.as_deref() else {
            continue;
        };
        let _ = BubbleOutputQueue::upsert_commentary_before_tool(
            messages,
            tool_call_id,
            row.text.clone(),
        );
    }
}

/// 锚定 open 旁白：写入 `turn-commentary-*`（工具可尚未存在）。
pub(super) fn try_reconcile_active_anchored_commentary(
    messages: &mut Vec<StoredMessage>,
    projection: &TurnProjection,
    loading_tail_id: Option<&str>,
) -> bool {
    let Some(active) = projection.active_row.as_ref() else {
        return false;
    };
    if active.kind != ASSISTANT_COMMENTARY {
        return false;
    }
    let Some(tcid) = active.before_tool_call_id.as_deref() else {
        return false;
    };
    BubbleOutputQueue::upsert_streaming_anchored_commentary(
        messages,
        tcid,
        active.text.clone(),
        loading_tail_id,
    )
}

/// 工具批结束后 upsert `turn-final-answer`（位于 loading 尾泡之前）。
///
/// 从 overlay 读取终答正文。调用方须已确认允许落盘终答（post-tool / on_done）。
pub(super) fn reconcile_final_answer_from_overlay(
    messages: &mut Vec<StoredMessage>,
    turn: &TurnCanonicalState,
    loading_tail_id: Option<&str>,
    overlay_answer: Option<&str>,
) {
    if turn.tool_phase_open() {
        return;
    }
    if commentary_projection_pending_in_messages(messages, turn) {
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
    let insert_idx = insert_index_for_final_row(messages, loading_tail_id);
    BubbleOutputQueue::upsert_assistant_row(messages, FINAL_ANSWER_ROW_ID, text, insert_idx);
}

/// 若 FINAL_ANSWER_ROW 缺失，从给定正文补建（Phase C `drain` 兜底）。
pub(super) fn ensure_final_answer_row_from_text(
    messages: &mut Vec<StoredMessage>,
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
    let insert_idx = insert_index_for_final_row(messages, loading_tail_id);
    BubbleOutputQueue::upsert_assistant_row(
        messages,
        FINAL_ANSWER_ROW_ID,
        trimmed.to_string(),
        insert_idx,
    );
}

/// `on_tool_call`：插入工具占位并将 loading 尾泡钉到列表末尾。
pub(super) fn insert_declared_tool(
    messages: &mut Vec<StoredMessage>,
    tool_msg: StoredMessage,
    subgoal_marker: Option<&str>,
    loading_tail_id: &str,
) {
    insert_tool_row(messages, tool_msg, subgoal_marker);
    pin_loading_tail_in_messages(messages, loading_tail_id);
}

pub(super) fn insert_tool_row(
    messages: &mut Vec<StoredMessage>,
    tool_msg: StoredMessage,
    subgoal_marker: Option<&str>,
) {
    if let Some(mk) = subgoal_marker
        && let Some(idx) = messages.iter().rposition(|m| {
            m.state
                .as_ref()
                .is_some_and(|st| st.matches_full_marker(mk))
        })
    {
        messages.insert(idx + 1, tool_msg);
    } else {
        messages.push(tool_msg);
    }
}

pub(super) fn pin_loading_tail_in_messages(messages: &mut Vec<StoredMessage>, loading_id: &str) {
    let Some(idx) = messages.iter().position(|m| m.id == loading_id) else {
        return;
    };
    if messages[idx].role != "assistant" || !stored_message_is_loading(&messages[idx]) {
        return;
    }
    let m = messages.remove(idx);
    messages.push(m);
}

fn commentary_projection_pending_in_messages(
    messages: &[StoredMessage],
    turn: &TurnCanonicalState,
) -> bool {
    project_turn_projection(turn.turn_ref())
        .finalized_rows
        .into_iter()
        .filter(|row| row.kind == ASSISTANT_COMMENTARY)
        .filter_map(|row| row.tool_call_id)
        .map(|tool_call_id| commentary_row_id(tool_call_id.as_str()))
        .any(|row_id| messages.iter().all(|message| message.id != row_id))
}

fn insert_index_for_final_row(messages: &[StoredMessage], loading_tail_id: Option<&str>) -> usize {
    let mut insert_idx =
        BubbleOutputQueue::insert_index_before_loading_tail(messages, loading_tail_id);
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
