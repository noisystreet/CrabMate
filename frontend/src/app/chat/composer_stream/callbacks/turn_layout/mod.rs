//! 单轮 `/chat/stream` 内 **`messages` 布局** 的唯一入口（方向 A：显式 TurnLayout 状态机）。
//!
//! 目标顺序（Phase 9 块布局）：`[时间线*] → [turn-batch-narration] → [工具*] → [turn-final-answer] → [loading 空壳]`
//! assistant 批说明 / 终答正文 **仅** 经 [`BubbleOutputQueue::sync_web_projection`] 落盘。
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
#[cfg(test)]
mod turn_web_contract;

use std::cell::RefCell;

use leptos::prelude::GetUntracked;

use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    stream_overlay_answer_for_message, stream_overlay_clear_answer_for_message,
    stream_overlay_replace_answer_for_message, stream_overlay_take_into_stored_message,
};

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::turn_canonical::TurnCanonicalState;

pub(crate) use bubble_queue::{BubbleOutputQueue, FINAL_ANSWER_ROW_ID};

/// post-tool 尾泡被提前 finalize 时暂存的总结正文。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeeledSummary {
    text: String,
    reasoning_text: String,
}

fn overlay_answer_for_loading_tail(
    stream_ctx: &ChatStreamCallbackCtx,
    loading_id: &str,
) -> Option<String> {
    stream_overlay_answer_for_message(
        stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
        stream_ctx.bound_stream_session_id.as_str(),
        loading_id,
    )
}

/// 工具边界：将 loading 尾泡 overlay 旁注提交进 canonical（P0′ 空壳 stored 时 peel 摘不到字）。
fn commit_overlay_commentary_to_canonical(stream_ctx: &ChatStreamCallbackCtx) {
    if !stream_ctx.scratch.tool_phase_open() && stream_ctx.scratch.post_tool_stream_tail_active() {
        // post-tool 终答 preview 在 overlay；勿误入批说明。
        return;
    }
    let mid = stream_ctx.scratch.clone_assistant_id();
    let Some(answer) = overlay_answer_for_loading_tail(stream_ctx, mid.as_str()) else {
        return;
    };
    if stream_ctx.scratch.tool_phase_open() {
        stream_ctx
            .scratch
            .ingest_batch_commentary_from_peel(answer.as_str());
    } else {
        stream_ctx
            .scratch
            .absorb_pre_tool_narration_for_first_tool(answer.as_str());
    }
}

/// 工具边界 / demote：overlay 与 loading stored 正文 **仅** 迁入 canonical，不写 `StoredMessage` 助手行。
pub(crate) fn drain_loading_commentary_to_canonical(stream_ctx: &ChatStreamCallbackCtx) {
    if !stream_ctx.scratch.tool_phase_open() && stream_ctx.scratch.post_tool_stream_tail_active() {
        return;
    }
    commit_overlay_commentary_to_canonical(stream_ctx);
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let drained = RefCell::new(None::<String>);
    stream_ctx.update_bound_session(|s| {
        let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) else {
            return;
        };
        stream_overlay_take_into_stored_message(
            stream_ctx.chat.stream_text_overlay,
            sid.as_str(),
            mid.as_str(),
            &mut s.messages[idx],
        );
        let text = s.messages[idx].text.trim();
        if !text.is_empty() {
            *drained.borrow_mut() = Some(s.messages[idx].text.clone());
        }
        s.messages[idx].text.clear();
    });
    if let Some(text) = drained.into_inner() {
        if stream_ctx.scratch.tool_phase_open() {
            stream_ctx
                .scratch
                .ingest_batch_commentary_from_peel(text.as_str());
        } else {
            stream_ctx
                .scratch
                .absorb_pre_tool_narration_for_first_tool(text.as_str());
        }
    }
    stream_overlay_clear_answer_for_message(
        stream_ctx.chat.stream_text_overlay,
        sid.as_str(),
        mid.as_str(),
        Some(stream_ctx.chat.stream_overlay_revision),
    );
}

fn discard_premature_assistant_tail(
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
    if m.role != "assistant" || m.is_tool || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
        return;
    }
    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
        return;
    }
    messages[idx].text.clear();
    messages[idx].reasoning_text.clear();
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
/// Phase 7 遗留：单测仍覆盖 peel 语义；生产路径 Phase 9 用 [`discard_premature_assistant_tail`]。
#[cfg(test)]
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
    /// 工具边界：overlay / loading stored → canonical（Phase 9；不写 stored 助手正文行）。
    pub(crate) fn drain_loading_commentary_to_canonical(stream_ctx: &ChatStreamCallbackCtx) {
        drain_loading_commentary_to_canonical(stream_ctx);
    }

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
        drain_loading_commentary_to_canonical(stream_ctx);
        stream_ctx.update_bound_session(|session| {
            let mid = stream_ctx.scratch.clone_assistant_id();
            let Some(idx) = session.messages.iter().position(|m| m.id == mid) else {
                return;
            };
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

    /// `on_tool_call`：插入工具占位 → 空 loading 尾泡（Phase 9：正文仅经 `sync_web_projection` 落盘）。
    pub(crate) fn on_tool_call_declared(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_msg: StoredMessage,
        subgoal_marker: Option<&str>,
    ) {
        let tool_id = tool_msg.id.clone();
        let mid = stream_ctx.scratch.clone_assistant_id();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            discard_premature_assistant_tail(&mut s.messages, mid.as_str());
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
        drain_loading_commentary_to_canonical(stream_ctx);
        let mid = stream_ctx.scratch.clone_assistant_id();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            discard_premature_assistant_tail(&mut s.messages, mid.as_str());
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
        stream_ctx.scratch.sync_turn_projection(stream_ctx);
        stream_ctx.scratch.sync_stream_preview(stream_ctx);
    }
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

    /// 流结束：若 `turn-final-answer` 已落盘且 loading 尾泡与其重复，去掉尾泡避免导出双段。
    pub(crate) fn dedupe_loading_tail_against_final_answer_row(
        messages: &mut Vec<StoredMessage>,
        loading_id: &str,
    ) {
        use crate::message_dedupe::assistant_texts_fuzzy_duplicate;

        let Some(final_idx) = messages
            .iter()
            .position(|m| m.id == bubble_queue::FINAL_ANSWER_ROW_ID)
        else {
            return;
        };
        let Some(load_idx) = messages.iter().position(|m| m.id == loading_id) else {
            return;
        };
        let final_text = messages[final_idx].text.as_str();
        let load = &messages[load_idx];
        if load.text.trim().is_empty() && load.reasoning_text.trim().is_empty() {
            messages.remove(load_idx);
            return;
        }
        if assistant_texts_fuzzy_duplicate(load.text.as_str(), final_text) {
            messages.remove(load_idx);
        }
    }

    /// 段/工具边界：flush 工具批说明块到 stored；未落盘前保留 overlay preview。
    pub(crate) fn sync_turn_projection(
        stream_ctx: &ChatStreamCallbackCtx,
        turn: &TurnCanonicalState,
        queue: &mut BubbleOutputQueue,
    ) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            queue.sync_web_projection(&mut s.messages, turn, Some(mid.as_str()));
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

/// 说明块已落盘或无需 preview 时，可安全清空 loading 尾泡 overlay answer。
pub(super) fn should_clear_preview_overlay_answer(
    turn: &TurnCanonicalState,
    messages: &[StoredMessage],
) -> bool {
    BubbleOutputQueue::loading_preview_for_messages(turn, messages).is_empty()
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
