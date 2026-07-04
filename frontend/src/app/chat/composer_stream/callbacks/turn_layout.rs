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
    demote_assistant_message_answer_to_commentary, stream_overlay_demote_answer_to_reasoning,
    stream_overlay_take_into_stored_message,
};

use crabmate_turn_layout::project_turn;

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

fn merge_summary_into_loading_row(m: &mut StoredMessage, peeled: &PeeledSummary) {
    if !peeled.text.is_empty() {
        if !m.text.is_empty() {
            m.text.push('\n');
        }
        m.text.push_str(&peeled.text);
    }
    if !peeled.reasoning_text.is_empty() {
        if !m.reasoning_text.is_empty() {
            m.reasoning_text.push('\n');
        }
        m.reasoning_text.push_str(&peeled.reasoning_text);
    }
}

/// 单轮流式会话的消息布局状态机（无独立字段：状态由 `messages` + scratch 共同表示）。
pub(crate) struct TurnLayout;

impl TurnLayout {
    /// 模型轮次确认含 `tool_calls`：将已流出的正文降级为 commentary 旁注。
    pub(crate) fn demote_answer_before_tools(
        stream_ctx: &ChatStreamCallbackCtx,
        accum: &PerStreamAccum,
    ) {
        stream_ctx.scratch.enter_commentary_before_tools_lane();
        let sid = stream_ctx.bound_stream_session_id.clone();
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_overlay_demote_answer_to_reasoning(
            stream_ctx.chat.stream_text_overlay,
            sid.as_str(),
            mid.as_str(),
        );
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
            let migrated = session.messages[idx].text.clone();
            if !migrated.trim().is_empty() {
                stream_ctx
                    .scratch
                    .ingest_pre_tool_commentary(migrated.as_str());
            }
            demote_assistant_message_answer_to_commentary(&mut session.messages[idx]);
            session.messages[idx].text.clear();
            session.messages[idx].reasoning_text.clear();
        });
        stream_ctx.scratch.sync_turn_projection(stream_ctx);
        accum.clear_answer_delta_chars();
    }

    /// `on_tool_call`：peel 过早总结 → 插入工具占位 → 开 post-tool loading 尾泡 → 恢复总结 → pin。
    pub(crate) fn on_tool_call_declared(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_msg: StoredMessage,
        subgoal_marker: Option<&str>,
    ) {
        let tool_id = tool_msg.id.clone();
        let peeled = Self::peel_premature_summary(stream_ctx);
        stream_ctx.update_bound_session(|s| {
            insert_tool_row(&mut s.messages, tool_msg, subgoal_marker);
        });
        Self::open_post_tool_loading_tail_after(stream_ctx, tool_id.as_str());
        if let Some(summary) = peeled {
            Self::restore_peeled_summary(stream_ctx, summary);
        }
        Self::pin_loading_tail(stream_ctx);
    }

    /// `tool_result` 在未命中占位时新建工具行后的布局收口。
    pub(crate) fn on_tool_result_inserted(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_message_id: &str,
    ) {
        let peeled = Self::peel_premature_summary(stream_ctx);
        if peeled.is_some() {
            Self::open_post_tool_loading_tail_after(stream_ctx, tool_message_id);
            if let Some(summary) = peeled {
                Self::restore_peeled_summary(stream_ctx, summary);
            }
        }
        Self::pin_loading_tail(stream_ctx);
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
            }
            let mid = stream_ctx.scratch.borrow_assistant_id();
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                let m = &mut s.messages[idx];
                if m.role == "assistant" && m.state.as_ref().is_some_and(|st| st.is_loading()) {
                    if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
                        s.messages.remove(idx);
                    } else {
                        m.state = None;
                    }
                }
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

    fn peel_premature_summary(stream_ctx: &ChatStreamCallbackCtx) -> Option<PeeledSummary> {
        let mid = stream_ctx.scratch.clone_assistant_id();
        let sid = stream_ctx.bound_stream_session_id.clone();
        let mut peeled = None;
        stream_ctx.update_bound_session(|s| {
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid.as_str(),
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            peeled = peel_premature_summary_from_messages(&mut s.messages, mid.as_str());
        });
        peeled
    }

    fn restore_peeled_summary(stream_ctx: &ChatStreamCallbackCtx, peeled: PeeledSummary) {
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) else {
                return;
            };
            if m.role != "assistant" || m.is_tool {
                return;
            }
            if !m.state.as_ref().is_some_and(|st| st.is_loading()) {
                return;
            }
            merge_summary_into_loading_row(m, &peeled);
        });
    }

    fn open_post_tool_loading_tail_after(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_message_id: &str,
    ) {
        let tool_present = stream_ctx
            .read_bound_session(|s| s.messages.iter().any(|m| m.id == tool_message_id))
            .unwrap_or(false);
        if !tool_present {
            return;
        }
        Self::finalize_loading_segment(stream_ctx);
        let now = message_created_ms();
        let new_tail_id = RefCell::new(None::<String>);
        stream_ctx.update_bound_session(|s| {
            let Some(tidx) = s.messages.iter().position(|m| m.id == tool_message_id) else {
                return;
            };
            let new_asst_id = make_message_id();
            s.messages.insert(
                tidx + 1,
                StoredMessage {
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
                },
            );
            *new_tail_id.borrow_mut() = Some(new_asst_id);
        });
        if let Some(id) = new_tail_id.into_inner() {
            stream_ctx
                .scratch
                .adopt_new_assistant_tail_after_rotation(id.clone());
            stream_ctx.chat.set_stream_overlay_display_mid(id.as_str());
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

impl TurnLayout {
    /// 按 [`project_turn`] 投影 upsert 各工具前旁注行。
    pub(crate) fn sync_turn_projection(
        stream_ctx: &ChatStreamCallbackCtx,
        turn: &TurnCanonicalState,
    ) {
        for row in project_turn(turn.turn_ref()) {
            if row.kind != "assistant_commentary" {
                continue;
            }
            let Some(tcid) = row.tool_call_id.as_deref().filter(|s| !s.is_empty()) else {
                continue;
            };
            Self::sync_commentary_before_tool_with_text(stream_ctx, tcid, row.text.as_str());
        }
    }

    fn sync_commentary_before_tool_with_text(
        stream_ctx: &ChatStreamCallbackCtx,
        tool_call_id: &str,
        text: &str,
    ) {
        if text.trim().is_empty() {
            return;
        }
        stream_ctx.update_bound_session(|s| {
            let Some(tool_idx) = find_tool_index(&s.messages, tool_call_id) else {
                return;
            };
            if let Some(cidx) =
                find_commentary_before_tool_index(&s.messages, tool_call_id, tool_idx)
            {
                s.messages[cidx].text = text.to_string();
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
            s.messages.insert(tool_idx, msg);
        });
        Self::pin_loading_tail(stream_ctx);
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
            if tail_trim.is_empty() {
                return;
            }
            for m in s.messages[..load_idx].iter().rev() {
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
                    || prior.ends_with(tail_trim.as_str())
                    || (tail_trim.len() > 40 && prior.contains(tail_trim.as_str()))
                {
                    s.messages[load_idx].text.clear();
                    s.messages[load_idx].reasoning_text.clear();
                }
                break;
            }
        });
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
mod tests {
    use super::*;

    fn empty_msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn peel_removes_finalized_post_tool_tail_only() {
        let mut msgs = vec![
            empty_msg("t0", "system", "tool", true),
            StoredMessage {
                id: "a_done".into(),
                role: "assistant".into(),
                text: "完成。".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        let peeled = peel_premature_summary_from_messages(&mut msgs, "a_done").expect("peeled");
        assert_eq!(
            peeled,
            PeeledSummary {
                text: "完成。".into(),
                reasoning_text: String::new(),
            }
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "t0");
    }

    #[test]
    fn peel_skips_loading_tail() {
        let mut msgs = vec![StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: "续写中".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }];
        assert!(peel_premature_summary_from_messages(&mut msgs, "a_load").is_none());
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn late_tool_order_tool_then_restored_summary() {
        let mut msgs = vec![
            empty_msg("t0", "system", "create file", true),
            StoredMessage {
                id: "a_done".into(),
                role: "assistant".into(),
                text: "完成。".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        let peeled = peel_premature_summary_from_messages(&mut msgs, "a_done").expect("peeled");
        msgs.push(empty_msg("t1", "system", "cmake", true));
        let mut loading = StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        merge_summary_into_loading_row(&mut loading, &peeled);
        msgs.push(loading);
        assert_eq!(msgs[0].id, "t0");
        assert_eq!(msgs[1].id, "t1");
        assert_eq!(msgs[2].id, "a_load");
        assert_eq!(msgs[2].text, "完成。");
    }
}
