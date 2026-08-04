//! 将 [`crabmate_turn_layout::TurnProjection`] 落到 `StoredMessage`，并承接 loading 行生命周期
//! （peel / finalize / rotate / 工具后新壳）的纯消息列表操作。
//!
//! Phase D：定稿旁白、锚定 active、终答 flush、工具占位插入均经本模块；
//! [`super::TurnLayout`] 只做 scratch / overlay / lane 编排。Loading 句柄不承载旁白/终答正文。

use crabmate_turn_layout::{ASSISTANT_COMMENTARY, TurnProjection, project_turn_projection};

use crate::message_loading::{
    is_finalized_plain_assistant, is_loading_plain_assistant, is_loading_streaming_assistant_id,
    is_plain_assistant_message, stored_message_is_loading,
};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};

use super::super::super::turn_canonical::TurnCanonicalState;
use super::loading_handoff;
use super::turn_row_queue::{
    FINAL_ANSWER_ROW_ID, TurnRowQueue, commentary_row_id, current_turn_position,
    is_commentary_row_id,
};

/// peel 下来的过早 finalize / loading 尾泡正文（表征测试用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PeeledSummary {
    pub text: String,
    pub reasoning_text: String,
}

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
        let _ =
            TurnRowQueue::upsert_commentary_before_tool(messages, tool_call_id, row.text.clone());
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
    TurnRowQueue::upsert_streaming_anchored_commentary(
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
    TurnRowQueue::upsert_assistant_row(messages, FINAL_ANSWER_ROW_ID, text, insert_idx);
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
    TurnRowQueue::upsert_assistant_row(
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
    loading_tail_id: &str,
) {
    insert_tool_row(messages, tool_msg);
    pin_loading_tail_in_messages(messages, loading_tail_id);
}

pub(super) fn insert_tool_row(messages: &mut Vec<StoredMessage>, tool_msg: StoredMessage) {
    messages.push(tool_msg);
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
        .any(|row_id| current_turn_position(messages, row_id.as_str()).is_none())
}

fn insert_index_for_final_row(messages: &[StoredMessage], loading_tail_id: Option<&str>) -> usize {
    let mut insert_idx = TurnRowQueue::insert_index_before_loading_tail(messages, loading_tail_id);
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

// —— loading 行生命周期（原 TurnLayout 内联，收口到 reconciler）——

/// 将 `turn-final-answer` 投影行脱钩为普通 assistant 行。
pub(super) fn detach_final_answer_row_in_messages(messages: &mut [StoredMessage]) {
    if let Some(idx) = messages.iter().position(|m| m.id == FINAL_ANSWER_ROW_ID) {
        messages[idx].id = make_message_id();
    }
}

/// 在 loading 尾泡之前插入消息（时间线旁注等）。
pub(super) fn insert_msg_before_loading_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
    msg: StoredMessage,
) {
    if let Some(idx) = messages
        .iter()
        .position(|m| is_loading_streaming_assistant_id(m, streaming_assistant_id))
    {
        messages.insert(idx, msg);
    } else {
        messages.push(msg);
    }
}

/// 摘下已提前 finalize 的 post-tool 尾泡。
pub(super) fn peel_premature_summary_from_messages(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
) -> Option<PeeledSummary> {
    let idx = messages
        .iter()
        .position(|m| m.id == streaming_assistant_id)?;
    if !is_finalized_plain_assistant(&messages[idx]) {
        return None;
    }
    let removed = messages.remove(idx);
    Some(PeeledSummary {
        text: removed.text,
        reasoning_text: removed.reasoning_text,
    })
}

/// 仅 peel 过早 finalize；否则清空仍 loading 且有正文的尾泡（不删行）。
pub(super) fn discard_premature_assistant_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
) {
    if peel_premature_summary_from_messages(messages, streaming_assistant_id).is_some() {
        return;
    }
    let Some(idx) = messages.iter().position(|m| m.id == streaming_assistant_id) else {
        return;
    };
    let m = &messages[idx];
    if !is_loading_streaming_assistant_id(m, streaming_assistant_id) {
        return;
    }
    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
        return;
    }
    messages[idx].text.clear();
    messages[idx].reasoning_text.clear();
}

/// 下一工具边界前摘下 post-tool 尾泡正文（表征测试 / Phase 7 遗留）。
#[cfg(test)]
pub(super) fn extract_post_tool_tail_before_tool(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
) -> Option<PeeledSummary> {
    if let Some(peeled) = peel_premature_summary_from_messages(messages, streaming_assistant_id) {
        return Some(peeled);
    }
    let idx = messages
        .iter()
        .position(|m| m.id == streaming_assistant_id)?;
    let m = &messages[idx];
    if m.role != "assistant" || m.is_tool {
        return None;
    }
    if !stored_message_is_loading(m) {
        return None;
    }
    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
        return None;
    }
    let removed = messages.remove(idx);
    Some(PeeledSummary {
        text: removed.text,
        reasoning_text: removed.reasoning_text,
    })
}

/// 结束指定下标的 loading 助手行：空壳删除，有正文则去 `Loading`。
pub(super) fn finalize_loading_row_at(messages: &mut Vec<StoredMessage>, idx: usize) {
    if idx >= messages.len() {
        return;
    }
    let m = &messages[idx];
    if m.role != "assistant" || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
        return;
    }
    let text_trim = m.text.trim().to_string();
    if !text_trim.is_empty()
        && loading_handoff::persisted_assistant_owns_live_text(messages, idx, text_trim.as_str())
    {
        messages[idx].text.clear();
    }
    let content_saved =
        !messages[idx].text.trim().is_empty() || !messages[idx].reasoning_text.trim().is_empty();
    if content_saved {
        messages[idx].state = None;
    } else {
        messages.remove(idx);
    }
}

/// 在工具行后插入空 loading 壳，并 pin 到末尾。
pub(super) fn insert_post_tool_loading_after_tool(
    messages: &mut Vec<StoredMessage>,
    tool_message_id: &str,
) -> Option<String> {
    let tidx = messages.iter().position(|m| m.id == tool_message_id)?;
    let new_asst_id = make_message_id();
    let row = StoredMessage {
        id: new_asst_id.clone(),
        role: "assistant".to_string(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: message_created_ms(),
    };
    messages.insert(tidx + 1, row);
    pin_loading_tail_in_messages(messages, new_asst_id.as_str());
    Some(new_asst_id)
}

/// 追加空 loading 助手行（rotate）。
pub(super) fn append_empty_loading_assistant(messages: &mut Vec<StoredMessage>) -> String {
    let new_asst_id = make_message_id();
    messages.push(StoredMessage {
        id: new_asst_id.clone(),
        role: "assistant".to_string(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: message_created_ms(),
    });
    new_asst_id
}

/// 移除仍处于 Loading 的普通助手行（`final_response` 提前撤壳）。
pub(super) fn remove_loading_plain_assistant_by_id(
    messages: &mut Vec<StoredMessage>,
    assistant_id: &str,
) -> bool {
    let Some(idx) = messages.iter().position(|m| m.id == assistant_id) else {
        return false;
    };
    if !is_loading_plain_assistant(&messages[idx]) {
        return false;
    }
    messages.remove(idx);
    true
}

/// 是否仍存在同 id 的普通助手行（非工具）。
pub(super) fn plain_assistant_id_present(messages: &[StoredMessage], assistant_id: &str) -> bool {
    messages
        .iter()
        .any(|m| m.id == assistant_id && is_plain_assistant_message(m))
}

/// 清空指定 loading 尾泡 stored 正文（`on_done` drain / reset）。
pub(super) fn clear_assistant_row_text(messages: &mut [StoredMessage], assistant_id: &str) {
    if let Some(idx) = messages.iter().position(|m| m.id == assistant_id) {
        messages[idx].text.clear();
    }
}

/// 流结束：若终答行已落盘且 loading 尾泡与任意助手行模糊重复，去掉尾泡。
pub(super) fn dedupe_loading_tail_against_final_answer_row(
    messages: &mut Vec<StoredMessage>,
    loading_id: &str,
) {
    use crate::message_dedupe::assistant_texts_fuzzy_duplicate;

    let Some(load_idx) = messages.iter().position(|m| m.id == loading_id) else {
        return;
    };
    let load_text = &messages[load_idx].text;
    if load_text.trim().is_empty() && messages[load_idx].reasoning_text.trim().is_empty() {
        messages.remove(load_idx);
        return;
    }
    let duplicate_found = messages.iter().any(|m| {
        if m.id == loading_id {
            return false;
        }
        if m.role != "assistant" || m.is_tool {
            return false;
        }
        assistant_texts_fuzzy_duplicate(load_text.as_str(), m.text.as_str())
    });
    if duplicate_found {
        messages.remove(load_idx);
    }
}

/// 流结束：已有 commentary 行时去掉仍含正文的 loading 尾泡。
pub(super) fn dedupe_loading_tail_against_commentary_rows(
    messages: &mut Vec<StoredMessage>,
    loading_id: &str,
) {
    let has_commentary = messages.iter().any(|message| {
        is_commentary_row_id(message.id.as_str()) && !message.text.trim().is_empty()
    });
    if !has_commentary {
        return;
    }
    let Some(load_idx) = messages.iter().position(|m| m.id == loading_id) else {
        return;
    };
    let load = &messages[load_idx];
    if load.text.trim().is_empty() && load.reasoning_text.trim().is_empty() {
        messages.remove(load_idx);
        return;
    }
    messages.remove(load_idx);
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::app::chat::composer_stream::callbacks::turn_layout::text_ownership;
    use crate::app::chat::composer_stream::callbacks::turn_layout::turn_row_queue::{
        FINAL_ANSWER_ROW_ID, TurnRowQueue,
    };
    use crate::app::chat::composer_stream::turn_canonical::TurnCanonicalState;
    use crate::sse_dispatch::TurnSegmentStartInfo;
    use crate::storage::StoredMessageState;

    fn loading(id: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    fn tool(id: &str, tcid: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "system".into(),
            text: "tool".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some(tcid.into()),
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn anchored_commentary_reconcile_leaves_loading_text_empty() {
        let mut turn = TurnCanonicalState::new();
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("旁白。"));
        turn.on_segment_end("seg".into());
        turn.on_tool_call("tc_a", "t", "t");

        let mut messages = vec![tool("t1", "tc_a"), loading("load")];
        reconcile_web_projection(&mut messages, &turn, Some("load"), None, false);
        assert!(
            text_ownership::duplicate_commentary_row_ids(&messages).is_empty(),
            "single commentary key"
        );
        assert!(
            !text_ownership::loading_holds_duplicate_of_persisted(&messages, "load"),
            "loading must not hold commentary text"
        );
        let load = messages.iter().find(|m| m.id == "load").expect("load");
        assert!(load.text.is_empty());
    }

    #[test]
    fn final_answer_reconcile_does_not_write_loading_text() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_phase_end();
        let mut messages = vec![loading("load")];
        reconcile_web_projection(&mut messages, &turn, Some("load"), Some("终答正文。"), true);
        assert_eq!(text_ownership::final_answer_row_count(&messages), 1);
        assert!(
            messages
                .iter()
                .any(|m| m.id == FINAL_ANSWER_ROW_ID && m.text == "终答正文。")
        );
        let load = messages.iter().find(|m| m.id == "load").expect("load");
        assert!(
            load.text.is_empty(),
            "final must land on turn-final-answer, not loading.text"
        );
    }

    #[test]
    fn upsert_same_commentary_key_does_not_duplicate_row() {
        let mut messages = vec![tool("t1", "tc_a"), loading("load")];
        assert!(TurnRowQueue::upsert_commentary_before_tool(
            &mut messages,
            "tc_a",
            "一。".into()
        ));
        assert!(TurnRowQueue::upsert_commentary_before_tool(
            &mut messages,
            "tc_a",
            "一。二。".into()
        ));
        assert!(text_ownership::duplicate_commentary_row_ids(&messages).is_empty());
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.id == text_ownership::expected_commentary_id("tc_a"))
                .count(),
            1
        );
    }
}
