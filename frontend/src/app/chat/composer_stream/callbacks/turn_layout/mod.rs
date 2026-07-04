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

use std::cell::RefCell;

use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::{
    stream_overlay_clear_answer_for_message, stream_overlay_take_into_stored_message,
};

use crabmate_turn_layout::{Turn, project_turn};

use crate::message_dedupe::assistant_texts_fuzzy_duplicate;
use crate::message_dedupe::dedupe_assistant_messages_since_last_user;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::turn_canonical::TurnCanonicalState;

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
            let _peeled = extract_post_tool_tail_before_tool(&mut s.messages, mid.as_str());
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
            let _peeled = extract_post_tool_tail_before_tool(&mut s.messages, mid.as_str());
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
}

fn find_tool_index(messages: &[StoredMessage], tool_call_id: &str) -> Option<usize> {
    messages
        .iter()
        .position(|m| m.is_tool && m.tool_call_id.as_deref() == Some(tool_call_id))
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
) {
    if text.trim().is_empty() {
        return;
    }
    let Some(tool_idx) = find_tool_index(messages, tool_call_id) else {
        return;
    };
    if let Some(cidx) = find_commentary_before_tool_index(messages, tool_call_id, tool_idx) {
        messages[cidx].text = text.to_string();
        return;
    }
    let msg = StoredMessage {
        id: make_message_id(),
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
}

fn sync_loading_tail_block_in_messages(
    messages: &mut [StoredMessage],
    streaming_assistant_id: &str,
    text: &str,
) {
    if let Some(idx) = messages
        .iter()
        .position(|m| m.id == streaming_assistant_id && m.role == "assistant" && !m.is_tool)
    {
        messages[idx].text = text.to_string();
    }
}

#[allow(clippy::ptr_arg)]
fn repair_commentary_rows_before_tools(messages: &mut Vec<StoredMessage>, turn: &Turn) {
    for step in &turn.steps {
        let Some(ref text) = step.before_commentary else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let Some(tool_idx) = find_tool_index(messages, step.tool_call_id.as_str()) else {
            continue;
        };
        if find_commentary_before_tool_index(messages, step.tool_call_id.as_str(), tool_idx)
            .is_some()
        {
            continue;
        }
        sync_commentary_before_tool_in_messages(messages, step.tool_call_id.as_str(), text);
    }
}

/// 删除 canonical 已投影到工具前的旁注在列表其它位置的残留（常见于 peel/尾泡 finalize 后）。
fn relocate_misplaced_commentary_rows(messages: &mut Vec<StoredMessage>, turn: &Turn) {
    for step in &turn.steps {
        let Some(ref text) = step.before_commentary else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let tcid = step.tool_call_id.as_str();
        let Some(tool_idx) = find_tool_index(messages, tcid) else {
            continue;
        };
        let mut i = 0;
        while i < messages.len() {
            let m = &messages[i];
            let remove = m.role == "assistant"
                && !m.is_tool
                && !(i + 1 == tool_idx && m.tool_call_id.as_deref() == Some(tcid))
                && i >= tool_idx
                && assistant_texts_fuzzy_duplicate(m.text.as_str(), text.as_str());
            if remove {
                messages.remove(i);
            } else {
                i += 1;
            }
        }
        let Some(tool_idx) = find_tool_index(messages, tcid) else {
            continue;
        };
        if let Some(cidx) = find_commentary_before_tool_index(messages, tcid, tool_idx) {
            messages[cidx].text = text.clone();
            messages[cidx].tool_call_id = Some(step.tool_call_id.clone());
        } else {
            sync_commentary_before_tool_in_messages(messages, tcid, text.as_str());
        }
    }
}

impl TurnLayout {
    /// 按 [`project_turn`] 投影 upsert 工具前旁注与终答行。
    pub(crate) fn sync_turn_projection(
        stream_ctx: &ChatStreamCallbackCtx,
        turn: &TurnCanonicalState,
    ) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            for row in project_turn(turn.turn_ref()) {
                match row.kind.as_str() {
                    "assistant_commentary" => {
                        let Some(tcid) = row.tool_call_id.as_deref().filter(|s| !s.is_empty())
                        else {
                            continue;
                        };
                        sync_commentary_before_tool_in_messages(
                            &mut s.messages,
                            tcid,
                            row.text.as_str(),
                        );
                    }
                    "assistant_answer" => {
                        if turn.tool_phase_open() {
                            continue;
                        }
                        sync_loading_tail_block_in_messages(
                            &mut s.messages,
                            mid.as_str(),
                            row.text.as_str(),
                        );
                    }
                    _ => {}
                }
            }
            if turn.tool_phase_open() {
                let block = turn.streaming_commentary_block_text().unwrap_or_default();
                sync_loading_tail_block_in_messages(&mut s.messages, mid.as_str(), block.as_str());
            }
            repair_commentary_rows_before_tools(&mut s.messages, turn.turn_ref());
            relocate_misplaced_commentary_rows(&mut s.messages, turn.turn_ref());
        });
        stream_overlay_clear_answer_for_message(
            stream_ctx.chat.stream_text_overlay,
            stream_ctx.bound_stream_session_id.as_str(),
            mid.as_str(),
            Some(stream_ctx.chat.stream_overlay_revision),
        );
        Self::pin_loading_tail(stream_ctx);
    }

    /// 流结束前：自最后 user 起删除 fuzzy 重复的 assistant 行。
    pub(crate) fn dedupe_assistant_duplicates_in_messages(messages: &mut Vec<StoredMessage>) {
        dedupe_assistant_messages_since_last_user(messages);
    }

    /// 流结束前：若 loading 尾泡正文已完整出现在先前 assistant 行中，删空尾泡避免重复终答。
    pub(crate) fn dedupe_redundant_loading_tail(stream_ctx: &ChatStreamCallbackCtx) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.clone();
        stream_ctx.update_bound_session(|s| {
            let Some(load_idx) = s.messages.iter().position(|m| m.id == mid.as_str()) else {
                return;
            };
            if !s.messages[load_idx]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
            {
                return;
            }
            stream_overlay_take_into_stored_message(
                stream_ctx.chat.stream_text_overlay,
                sid.as_str(),
                mid.as_str(),
                &mut s.messages[load_idx],
            );
            let tail_trim = s.messages[load_idx].text.trim().to_string();
            let _ = remove_redundant_loading_tail_at(&mut s.messages, load_idx, tail_trim.as_str());
        });
    }
}

/// 若 loading 尾泡正文与最近一条可见 assistant 重复，移除该尾泡（canonical 投影已写入先行行）。
pub(crate) fn remove_redundant_loading_tail_at(
    messages: &mut Vec<StoredMessage>,
    load_idx: usize,
    tail_trim: &str,
) -> bool {
    if tail_trim.is_empty() {
        return false;
    }
    for m in messages[..load_idx].iter().rev() {
        if m.role != "assistant" || m.is_tool {
            continue;
        }
        if m.state.as_ref().is_some_and(|st| st.is_loading()) {
            continue;
        }
        let prior = m.text.trim();
        if prior.is_empty() {
            continue;
        }
        if prior == tail_trim
            || prior.ends_with(tail_trim)
            || assistant_texts_fuzzy_duplicate(prior, tail_trim)
        {
            messages.remove(load_idx);
            return true;
        }
        break;
    }
    false
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
