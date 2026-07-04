//! 单轮 `/chat/stream` 内 **`messages` 布局** 的唯一入口（方向 A：显式 TurnLayout 状态机）。
//!
//! 目标顺序：`[工具前旁注/时间线*] → [工具*] → [post-tool 总结 loading 尾泡]`
//!
//! | 事件 | 入口 |
//! |------|------|
//! | `tool_call` / `parsing_tool_calls` | [`TurnLayout::demote_answer_before_tools`] |
//! | `tool_call` 占位落盘 | [`TurnLayout::on_tool_call_declared`] |
//! | `tool_result` 新建行 | [`TurnLayout::on_tool_result_inserted`] |
//! | 时间线 / 意图 / 规划旁注 | [`TurnLayout::push_assistant_timeline`] |
//! | 分阶段 system 时间线 push | [`TurnLayout::after_auxiliary_system_push`] |
//! | 无工具的多轮 `assistant_answer_phase` | [`TurnLayout::rotate_followup_model_round`] |
//! | `final_response` 撤 loading | [`TurnLayout::remove_loading_placeholder_or_rotate`] |
//!
//! 原先分散在 `timeline_tail` 的 `peel` / `finalize` / `ensure_tail` / `restore` 均收拢为本模块私有步骤。

mod bubble_queue;

use std::cell::RefCell;

use leptos::prelude::GetUntracked;

use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    stream_overlay_clear_answer_for_message, stream_overlay_replace_answer_for_message,
    stream_overlay_take_into_stored_message,
};

use crabmate_turn_layout::commentary_for_tool;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::turn_canonical::TurnCanonicalState;

pub(crate) use bubble_queue::BubbleOutputQueue;

/// post-tool 尾泡被提前 finalize 时暂存的总结正文。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeeledSummary {
    text: String,
    reasoning_text: String,
}

fn is_premature_finalized_post_tool_tail(m: &StoredMessage) -> bool {
    m.role == "assistant" && !m.is_tool && !m.state.as_ref().is_some_and(|st| st.is_loading())
}

fn insert_msg_before_loading_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
    msg: StoredMessage,
) {
    if let Some(idx) = messages.iter().position(|m| {
        m.id == streaming_assistant_id
            && m.role == "assistant"
            && m.state.as_ref().is_some_and(|s| s.is_loading())
    }) {
        messages.insert(idx, msg);
    } else {
        messages.push(msg);
    }
}

fn peel_premature_summary_from_messages(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
) -> Option<PeeledSummary> {
    let idx = messages
        .iter()
        .position(|m| m.id == streaming_assistant_id)?;
    if !is_premature_finalized_post_tool_tail(&messages[idx]) {
        return None;
    }
    let removed = messages.remove(idx);
    Some(PeeledSummary {
        text: removed.text,
        reasoning_text: removed.reasoning_text,
    })
}

/// 下一工具边界前摘下 post-tool 尾泡正文：过早 finalize 行，或延迟 finalize 下仍 loading 的正文尾泡。
fn extract_post_tool_tail_before_tool(
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
    if !m.state.as_ref().is_some_and(|st| st.is_loading()) {
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

fn insert_tool_row(
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

/// 结束 loading 行：空则删，否则去 `loading` state（原则 B：不留空壳）。
fn finalize_loading_row_at(messages: &mut Vec<StoredMessage>, idx: usize) {
    if idx >= messages.len() {
        return;
    }
    let m = &messages[idx];
    if m.role != "assistant" || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
        return;
    }
    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
        messages.remove(idx);
    } else {
        messages[idx].state = None;
    }
}

fn pin_loading_tail_in_messages(messages: &mut Vec<StoredMessage>, loading_id: &str) {
    let Some(idx) = messages.iter().position(|m| m.id == loading_id) else {
        return;
    };
    if messages[idx].role != "assistant"
        || !messages[idx]
            .state
            .as_ref()
            .is_some_and(|st| st.is_loading())
    {
        return;
    }
    let m = messages.remove(idx);
    messages.push(m);
}

fn insert_post_tool_loading_after_tool(
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

/// 单轮流式会话的消息布局状态机（无独立字段：状态由 `messages` + scratch 共同表示）。
pub(crate) struct TurnLayout;

impl TurnLayout {
    /// 尾泡正文已与 `final_response` 一致时是否应立即 finalize（post-tool 阶段为 false，延迟 finalize）。
    pub(crate) fn should_finalize_loading_when_tail_matches_final_response(
        post_tool_stream_tail_active: bool,
    ) -> bool {
        !post_tool_stream_tail_active
    }

    /// `final_response` 到达且尾泡已有同文时：post-tool 阶段延迟 finalize，避免总结定稿后又被 peel。
    pub(crate) fn should_defer_finalize_on_final_response(
        stream_ctx: &ChatStreamCallbackCtx,
    ) -> bool {
        !Self::should_finalize_loading_when_tail_matches_final_response(
            stream_ctx.scratch.post_tool_stream_tail_active(),
        )
    }

    /// 模型轮次确认含 `tool_calls`：将已流出的正文降级为 commentary 旁注。
    ///
    /// **仅首个 `tool_call` 前**执行：post-tool 尾泡正文属于 [`AnswerDelta`] / 终答，不得再迁入 pending 旁注。
    pub(crate) fn demote_answer_before_tools(
        stream_ctx: &ChatStreamCallbackCtx,
        accum: &PerStreamAccum,
    ) {
        if stream_ctx.scratch.post_tool_stream_tail_active() {
            return;
        }
        stream_ctx.scratch.enter_commentary_before_tools_lane();
        let sid = stream_ctx.bound_stream_session_id.clone();
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|session| {
            let Some(idx) = session.messages.iter().position(|m| m.id == mid) else {
                return;
            };
            stream_overlay_take_into_stored_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid.as_str(),
                &mut session.messages[idx],
            );
            let migrated = if !session.messages[idx].text.trim().is_empty() {
                session.messages[idx].text.clone()
            } else {
                session.messages[idx].reasoning_text.clone()
            };
            stream_ctx
                .scratch
                .absorb_pre_tool_narration_for_first_tool(migrated.as_str());
            stream_overlay_clear_answer_for_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid.as_str(),
                Some(stream_ctx.chat.stream_overlay_revision),
            );
            session.messages[idx].text.clear();
            session.messages[idx].reasoning_text.clear();
            if session.messages[idx]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
            {
                session.messages.remove(idx);
            }
        });
        accum.clear_answer_delta_chars();
    }

    /// `on_tool_call`：peel 过早总结 → 插入工具占位 → 开 post-tool loading 尾泡 → 恢复总结 → pin。
    pub(crate) fn on_tool_call_declared(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_msg: StoredMessage,
        subgoal_marker: Option<&str>,
    ) {
        let tool_id = tool_msg.id.clone();
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.clone();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            let peeled = extract_post_tool_tail_before_tool(&mut s.messages, mid.as_str());
            if let Some(ref summary) = peeled {
                if let Some(tcid) = tool_msg.tool_call_id.as_deref().filter(|t| !t.is_empty()) {
                    stream_ctx
                        .scratch
                        .ingest_commentary_for_tool_from_peel(tcid, summary.text.as_str());
                } else {
                    stream_ctx
                        .scratch
                        .ingest_pending_stream_commentary(summary.text.as_str());
                }
            }
            insert_tool_row(&mut s.messages, tool_msg, subgoal_marker);
            if let Some(load_idx) = s.messages.iter().position(|m| m.id == mid) {
                finalize_loading_row_at(&mut s.messages, load_idx);
            }
            if let Some(id) = insert_post_tool_loading_after_tool(&mut s.messages, tool_id.as_str())
            {
                *new_tail_id.borrow_mut() = Some(id);
            }
        });
        if let Some(id) = new_tail_id.into_inner() {
            stream_ctx
                .scratch
                .adopt_new_assistant_tail_after_rotation(id.clone());
            stream_ctx.chat.set_stream_overlay_display_mid(id.as_str());
        }
    }

    /// `tool_result` 在未命中占位时新建工具行后的布局收口。
    pub(crate) fn on_tool_result_inserted(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_message_id: &str,
    ) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.clone();
        let tcid_for_peel = stream_ctx
            .read_bound_session(|s| {
                s.messages
                    .iter()
                    .find(|m| m.id == tool_message_id)
                    .and_then(|m| m.tool_call_id.clone())
                    .filter(|t| !t.trim().is_empty())
            })
            .flatten();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            let peeled = extract_post_tool_tail_before_tool(&mut s.messages, mid.as_str());
            if let Some(ref summary) = peeled {
                if let Some(ref tcid) = tcid_for_peel {
                    stream_ctx
                        .scratch
                        .ingest_commentary_for_tool_from_peel(tcid.as_str(), summary.text.as_str());
                } else {
                    stream_ctx
                        .scratch
                        .ingest_pending_stream_commentary(summary.text.as_str());
                }
            }
            if s.messages.iter().all(|m| m.id != mid) {
                return;
            }
            if let Some(load_idx) = s.messages.iter().position(|m| m.id == mid) {
                finalize_loading_row_at(&mut s.messages, load_idx);
            }
            if let Some(id) = insert_post_tool_loading_after_tool(&mut s.messages, tool_message_id)
            {
                *new_tail_id.borrow_mut() = Some(id);
            }
        });
        if let Some(id) = new_tail_id.into_inner() {
            stream_ctx
                .scratch
                .adopt_new_assistant_tail_after_rotation(id.clone());
            stream_ctx.chat.set_stream_overlay_display_mid(id.as_str());
        } else {
            Self::pin_loading_tail(stream_ctx);
        }
        stream_ctx.scratch.sync_turn_projection(stream_ctx, true);
    }

    /// 新 commentary 段开始：清空 loading 尾泡上的流式正文，避免上一块残留。
    pub(crate) fn reset_loading_tail_streaming_text(stream_ctx: &ChatStreamCallbackCtx) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.clone();
        stream_ctx.update_bound_session(|s| {
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                s.messages[idx].text.clear();
            }
        });
        stream_overlay_clear_answer_for_message(
            stream_ctx.chat.stream_text_overlay,
            sid.as_str(),
            mid.as_str(),
            Some(stream_ctx.chat.stream_overlay_revision),
        );
    }

    /// 任意后续 `push`（时间线等）之后，保证 post-tool `loading` 尾泡仍在列表最末。
    pub(crate) fn pin_loading_tail(stream_ctx: &ChatStreamCallbackCtx) {
        if !stream_ctx.scratch.post_tool_stream_tail_active() {
            return;
        }
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            let Some(idx) = s.messages.iter().position(|m| m.id == mid) else {
                return;
            };
            if s.messages[idx].role != "assistant"
                || !s.messages[idx]
                    .state
                    .as_ref()
                    .is_some_and(|st| st.is_loading())
            {
                return;
            }
            if idx == s.messages.len().saturating_sub(1) {
                return;
            }
            let m = s.messages.remove(idx);
            s.messages.push(m);
        });
    }

    /// 助手时间线旁注（意图、规划、`final_response` 等）：插在 loading 尾泡前并 pin。
    pub(crate) fn push_assistant_timeline(stream_ctx: &ChatStreamCallbackCtx, msg: StoredMessage) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            insert_msg_before_loading_tail(&mut s.messages, mid.as_str(), msg);
        });
        Self::pin_loading_tail(stream_ctx);
    }

    /// 分阶段 system 时间线 `push` 到末尾后 pin loading 尾泡。
    pub(crate) fn after_auxiliary_system_push(stream_ctx: &ChatStreamCallbackCtx) {
        Self::pin_loading_tail(stream_ctx);
    }

    /// 结束当前 loading 助手段（空则删，否则去 `loading` state）。
    pub(crate) fn finalize_loading_segment(stream_ctx: &ChatStreamCallbackCtx) {
        let sid = stream_ctx.bound_stream_session_id.clone();
        stream_ctx.update_bound_session(|s| {
            let mid_owned = stream_ctx.scratch.clone_assistant_id();
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid_owned.as_str()) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid_owned.as_str(),
                    &mut s.messages[idx],
                );
                finalize_loading_row_at(&mut s.messages, idx);
            }
        });
    }

    /// 无工具的多轮 model round：finalize → 新 loading 尾泡 → pin。
    pub(crate) fn rotate_followup_model_round(stream_ctx: &ChatStreamCallbackCtx) {
        Self::finalize_loading_segment(stream_ctx);
        let now = message_created_ms();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            let new_asst_id = make_message_id();
            s.messages.push(StoredMessage {
                id: new_asst_id.clone(),
                role: "assistant".to_string(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: now,
            });
            *new_tail_id.borrow_mut() = Some(new_asst_id);
        });
        if let Some(id) = new_tail_id.into_inner() {
            stream_ctx
                .scratch
                .adopt_new_assistant_tail_after_rotation(id.clone());
            stream_ctx.chat.set_stream_overlay_display_mid(id.as_str());
        }
        Self::pin_loading_tail(stream_ctx);
    }

    /// `final_response` 等提前撤 loading；若尾泡已不存在则轮换新占位。
    pub(crate) fn remove_loading_placeholder_or_rotate(stream_ctx: &ChatStreamCallbackCtx) {
        let sid = stream_ctx.bound_stream_session_id.clone();
        let mid_owned = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid_owned.as_str())
                && s.messages[idx].role == "assistant"
                && s.messages[idx]
                    .state
                    .as_ref()
                    .is_some_and(|st| st.is_loading())
            {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid_owned.as_str(),
                    &mut s.messages[idx],
                );
                s.messages.remove(idx);
            }
        });
        let tail_still_present = stream_ctx
            .read_bound_session(|s| {
                s.messages
                    .iter()
                    .any(|m| m.id == mid_owned.as_str() && m.role == "assistant" && !m.is_tool)
            })
            .unwrap_or(false);
        if !tail_still_present {
            Self::rotate_followup_model_round(stream_ctx);
        }
    }

    /// **热路径**：canonical open 段 preview → overlay replace；**不** `sessions.update`、不 insert 旁注行。
    pub(crate) fn sync_stream_preview(
        stream_ctx: &ChatStreamCallbackCtx,
        turn: &TurnCanonicalState,
    ) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.as_str();
        let preview = stream_ctx
            .read_bound_session(|s| {
                BubbleOutputQueue::loading_preview_for_messages(turn, &s.messages)
            })
            .unwrap_or_default();
        let overlay = stream_ctx.chat.stream_text_overlay.get_untracked();
        let unchanged = overlay.as_ref().is_some_and(|o| {
            o.session_id == sid && o.message_id == mid.as_str() && o.answer == preview
        });
        if unchanged {
            return;
        }
        stream_overlay_replace_answer_for_message(
            stream_ctx.chat.stream_text_overlay,
            sid,
            mid.as_str(),
            preview.as_str(),
            Some(stream_ctx.chat.stream_overlay_revision),
        );
        stream_ctx.chat.set_stream_overlay_display_mid(mid.as_str());
    }

    /// 段/工具边界：flush 完整旁注到 stored；旁注未落盘前保留 overlay preview。
    pub(crate) fn sync_turn_projection(
        stream_ctx: &ChatStreamCallbackCtx,
        turn: &TurnCanonicalState,
        queue: &mut BubbleOutputQueue,
        relocate_stray: bool,
    ) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            queue.flush_complete_commentary_rows(&mut s.messages, turn);
            if relocate_stray {
                pin_commentary_rows_before_anchored_tools(&mut s.messages);
            }
        });
        let clear_overlay = stream_ctx
            .read_bound_session(|s| should_clear_preview_overlay_answer(turn, &s.messages))
            .unwrap_or(false);
        if clear_overlay {
            stream_overlay_clear_answer_for_message(
                stream_ctx.chat.stream_text_overlay,
                stream_ctx.bound_stream_session_id.as_str(),
                mid.as_str(),
                Some(stream_ctx.chat.stream_overlay_revision),
            );
        }
        Self::pin_loading_tail(stream_ctx);
    }
}

fn find_tool_index_latest(messages: &[StoredMessage], tool_call_id: &str) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tool_call_id))
}

fn find_tool_index(messages: &[StoredMessage], tool_call_id: &str) -> Option<usize> {
    find_tool_index_latest(messages, tool_call_id)
}

fn commentary_row_id(tool_call_id: &str) -> String {
    format!("commentary-before-{tool_call_id}")
}

fn find_commentary_before_tool_index(
    messages: &[StoredMessage],
    tool_call_id: &str,
    tool_idx: usize,
) -> Option<usize> {
    if tool_idx == 0 {
        return None;
    }
    let prev = &messages[tool_idx - 1];
    if prev.role == "assistant"
        && !prev.is_tool
        && prev.tool_call_id.as_deref() == Some(tool_call_id)
    {
        Some(tool_idx - 1)
    } else {
        None
    }
}

fn sync_commentary_before_tool_in_messages(
    messages: &mut Vec<StoredMessage>,
    tool_call_id: &str,
    text: &str,
) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let stable_id = commentary_row_id(tool_call_id);
    if let Some(cidx) = messages.iter().position(|m| m.id == stable_id) {
        if messages[cidx].text != text {
            messages[cidx].text = text.to_string();
        }
        if messages[cidx].tool_call_id.as_deref() != Some(tool_call_id) {
            messages[cidx].tool_call_id = Some(tool_call_id.to_string());
        }
        return true;
    }
    let Some(tool_idx) = find_tool_index(messages, tool_call_id) else {
        return false;
    };
    if let Some(cidx) = find_commentary_before_tool_index(messages, tool_call_id, tool_idx) {
        if messages[cidx].text != text {
            messages[cidx].text = text.to_string();
        }
        messages[cidx].id = stable_id;
        if messages[cidx].tool_call_id.as_deref() != Some(tool_call_id) {
            messages[cidx].tool_call_id = Some(tool_call_id.to_string());
        }
        return true;
    }
    let msg = StoredMessage {
        id: stable_id,
        role: "assistant".to_string(),
        text: text.to_string(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: Some(tool_call_id.to_string()),
        tool_name: None,
        created_at: message_created_ms(),
    };
    messages.insert(tool_idx, msg);
    true
}

/// 该工具前旁注是否已作为独立 assistant 行存在于 `messages` 中。
fn is_commentary_materialized_for_tool(messages: &[StoredMessage], tool_call_id: &str) -> bool {
    let stable_id = commentary_row_id(tool_call_id);
    if messages.iter().any(|m| m.id == stable_id) {
        return true;
    }
    let Some(tool_idx) = find_tool_index(messages, tool_call_id) else {
        return false;
    };
    find_commentary_before_tool_index(messages, tool_call_id, tool_idx).is_some()
}

/// canonical 中已有 closed 旁注、却尚未落为 stored 行（常见于 `segment_end` 早于 `tool_call`）。
pub(super) fn has_unmaterialized_commentary_rows(
    turn: &TurnCanonicalState,
    messages: &[StoredMessage],
) -> bool {
    use crabmate_turn_layout::SegmentKind;

    let mut anchors = std::collections::HashSet::new();
    for step in &turn.turn_ref().steps {
        anchors.insert(step.tool_call_id.clone());
    }
    for seg in &turn.turn_ref().segments {
        if seg.kind == SegmentKind::Commentary
            && !seg.open
            && let Some(tcid) = seg.before_tool_call_id.as_ref()
            && !tcid.is_empty()
        {
            anchors.insert(tcid.clone());
        }
    }
    for tcid in anchors {
        if turn.has_open_commentary_segment_for_tool(tcid.as_str()) {
            continue;
        }
        let Some(text) = commentary_for_tool(turn.turn_ref(), tcid.as_str()) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        if !is_commentary_materialized_for_tool(messages, tcid.as_str()) {
            return true;
        }
    }
    false
}

/// 旁注已落盘或无需 preview 时，可安全清空 loading 尾泡 overlay answer。
pub(super) fn should_clear_preview_overlay_answer(
    turn: &TurnCanonicalState,
    messages: &[StoredMessage],
) -> bool {
    if has_unmaterialized_commentary_rows(turn, messages) {
        return false;
    }
    BubbleOutputQueue::loading_preview_for_messages(turn, messages).is_empty()
}

/// 将带 `tool_call_id` 锚点、却漂在列表末尾（loading 尾泡前）的旁注行移回对应工具正前方。
fn pin_commentary_rows_before_anchored_tools(messages: &mut Vec<StoredMessage>) {
    let mut scan = 0usize;
    while scan < messages.len() {
        if messages[scan].role != "assistant"
            || messages[scan].is_tool
            || messages[scan]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
        {
            scan += 1;
            continue;
        }
        let Some(tcid) = messages[scan]
            .tool_call_id
            .clone()
            .filter(|s| !s.is_empty())
        else {
            scan += 1;
            continue;
        };
        let Some(tool_idx) = find_tool_index_latest(messages, tcid.as_str()) else {
            scan += 1;
            continue;
        };
        if tool_idx > 0 && messages[tool_idx - 1].id == messages[scan].id {
            scan += 1;
            continue;
        }
        let row = messages.remove(scan);
        let tool_idx = find_tool_index_latest(messages, tcid.as_str()).unwrap_or(tool_idx);
        messages.insert(tool_idx, row);
    }
}

#[cfg_attr(not(test), expect(dead_code))]
fn sync_loading_tail_block_in_messages(
    messages: &mut [StoredMessage],
    streaming_assistant_id: &str,
    text: &str,
) {
    if let Some(idx) = messages
        .iter()
        .position(|m| m.id == streaming_assistant_id && m.role == "assistant" && !m.is_tool)
    {
        if messages[idx].text == text {
            return;
        }
        messages[idx].text = text.to_string();
    }
}

/// 供 [`super::callbacks::helpers::timeline_tail`] 子目标 upsert 使用。
pub(crate) fn insert_assistant_before_loading_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
    msg: StoredMessage,
) {
    insert_msg_before_loading_tail(messages, streaming_assistant_id, msg);
}

#[cfg(test)]
mod tests;
