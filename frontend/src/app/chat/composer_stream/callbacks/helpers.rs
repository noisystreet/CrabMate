//! 时间线/气泡/子目标等辅助函数（供 [`super::builders`] 与 [`build_chat_stream_callbacks`] 使用）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crabmate_sse_protocol::StreamEndReason;
use leptos::prelude::*;

use crate::i18n;
use crate::i18n::Locale;
use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

use super::super::context::ChatStreamCallbackCtx;
use super::stream_session_access::{with_active_session_mut, with_active_session_ref};

pub(super) fn enqueue_pending_tool_message_id(
    queue: &Rc<RefCell<VecDeque<String>>>,
    message_id: String,
) {
    queue.borrow_mut().push_back(message_id);
}

pub(super) fn take_pending_tool_message_id(
    queue: &Rc<RefCell<VecDeque<String>>>,
) -> Option<String> {
    queue.borrow_mut().pop_front()
}

pub(super) fn non_empty_trimmed_tool_name(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub(super) fn build_final_response_text(title: &str, detail: Option<&str>) -> String {
    let mut final_text = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !final_text.is_empty() {
            final_text.push_str("\n\n");
        }
        final_text.push_str(detail);
    }
    final_text
}

pub(super) fn build_intent_analysis_main_bubble_text(title: &str, detail: Option<&str>) -> String {
    let title = title.trim();
    let detail = detail.map(str::trim).unwrap_or("");
    let mut out = String::new();
    if !title.is_empty() {
        out.push_str(title);
    }
    if !detail.is_empty() {
        let mut confidence = String::new();
        let mut primary = String::new();
        let mut clarification = String::new();
        let mut l2 = String::new();
        for line in detail.lines().map(str::trim) {
            match i18n::classify_intent_detail_line(line) {
                Some(i18n::IntentDetailLineKind::Confidence) => confidence = line.to_string(),
                Some(i18n::IntentDetailLineKind::PrimaryIntent) => primary = line.to_string(),
                Some(i18n::IntentDetailLineKind::NeedClarification) => {
                    clarification = line.to_string();
                }
                Some(i18n::IntentDetailLineKind::L2Result) => l2 = line.to_string(),
                None => {}
            }
        }
        let concise = [confidence, primary, clarification, l2]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !concise.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&concise);
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(super) fn build_hierarchical_plan_main_bubble_text(
    title: &str,
    detail: Option<&str>,
) -> String {
    let mut out = String::new();
    let title = title.trim();
    if !title.is_empty() {
        out.push_str(title);
    }
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(detail);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(super) fn build_hierarchical_subgoal_main_bubble_text(
    title: &str,
    detail: Option<&str>,
) -> String {
    let mut out = title.trim().to_string();
    if let Some(detail) = detail.map(str::trim)
        && !detail.is_empty()
    {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(detail);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n\n")
    }
}

pub(super) fn to_single_line(s: &str, max_chars: usize) -> String {
    let compact = s
        .split_whitespace()
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

/// 将旁注插在**当前流式 `loading` 助手气泡之前**；若无占位则追加到末尾。
pub(super) fn insert_msg_before_streaming_assistant_tail(
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

/// 管理器时间线（意图分析、规划摘要等）在服务端往往早于正文 `delta`，
/// 须插在**当前流式 `loading` 助手气泡之前**，否则会跑到已流出的计划文字下面。
pub(super) fn insert_before_streaming_assistant_or_append(
    stream_ctx: &ChatStreamCallbackCtx,
    msg: StoredMessage,
) {
    let mid = stream_ctx.tail.clone_assistant_id();
    with_active_session_mut(stream_ctx, |s| {
        insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
    });
}

pub(super) fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<StoredMessageState>,
) {
    if text.trim().is_empty() {
        return;
    }
    let now = message_created_ms();
    let msg = StoredMessage {
        id: make_message_id(),
        role: "assistant".to_string(),
        text,
        reasoning_text: String::new(),
        image_urls: vec![],
        state,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: now,
    };
    insert_before_streaming_assistant_or_append(stream_ctx, msg);
    ensure_streaming_assistant_tail_last(stream_ctx);
}

pub(super) fn has_same_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: &str,
) -> bool {
    with_active_session_ref(stream_ctx, |s| {
        s.messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !m.is_tool && m.state.is_none())
            .is_some_and(|m| m.text.trim() == text.trim())
    })
    .unwrap_or(false)
}

pub(super) fn extract_subgoal_marker_from_title(title: &str) -> Option<String> {
    let title = title.trim();
    for prefix in i18n::hierarchical_subgoal_title_prefixes() {
        if !title.starts_with(prefix) {
            continue;
        }
        let rest = title.strip_prefix(prefix)?;
        let goal_id = rest.strip_suffix('`')?;
        if goal_id.is_empty() {
            return None;
        }
        return Some(format!("hierarchical-subgoal:{goal_id}"));
    }
    None
}

pub(super) fn extract_subgoal_target_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| i18n::hierarchical_goal_target_raw(line).map(|_| line.to_string()))
}

pub(super) fn merge_subgoal_text_preserving_target(existing: &str, incoming: &str) -> String {
    if extract_subgoal_target_line(incoming).is_some() {
        return incoming.to_string();
    }
    let Some(target_line) = extract_subgoal_target_line(existing) else {
        return incoming.to_string();
    };
    let mut lines = incoming.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return format!("{target_line}\n\n");
    }
    let first_trim = lines[0].trim_start();
    let insert_idx = if i18n::hierarchical_subgoal_title_second_line_prefixes()
        .iter()
        .any(|p| first_trim.starts_with(p))
    {
        1
    } else {
        0
    };
    lines.insert(insert_idx, target_line.as_str());
    let mut out = lines.join("\n");
    if !out.ends_with("\n\n") {
        out.push_str("\n\n");
    }
    out
}

pub(super) fn upsert_hierarchical_subgoal_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    title: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    let marker = extract_subgoal_marker_from_title(title);
    if marker.is_none() {
        push_assistant_timeline_bubble(stream_ctx, text, None);
        return;
    }
    let marker = marker.unwrap_or_default();
    let now = message_created_ms();
    with_active_session_mut(stream_ctx, |s| {
        if let Some(existing) = s.messages.iter_mut().find(|m| {
            m.role == "assistant"
                && m.state
                    .as_ref()
                    .is_some_and(|st| st.matches_full_marker(marker.as_str()))
        }) {
            existing.text = merge_subgoal_text_preserving_target(&existing.text, &text);
            existing.created_at = now;
            return;
        }
        let msg = StoredMessage {
            id: make_message_id(),
            role: "assistant".to_string(),
            text: text.clone(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::HierarchicalSubgoal(marker.clone())),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        };
        let mid = stream_ctx.tail.clone_assistant_id();
        insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
    });
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 结束当前 `assistant_message_id` 指向的流式 `loading` 助手行：空正文则删除，否则去掉 `loading` state。
///
/// 供工具前收尾与无工具多轮轮换共用，避免两处复制分叉。
pub(super) fn finalize_current_loading_streaming_assistant_row(stream_ctx: &ChatStreamCallbackCtx) {
    with_active_session_mut(stream_ctx, |s| {
        let mid = stream_ctx.tail.borrow_assistant_id();
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            let m = &mut s.messages[idx];
            if m.role == "assistant" && m.state.as_ref().is_some_and(|s| s.is_loading()) {
                if m.text.trim().is_empty() {
                    s.messages.remove(idx);
                } else {
                    m.state = None;
                }
            }
        }
    });
}

/// 工具卡片插入前：结束当前流式段（开场白等保留在工具与时间线**之上**），
/// 并在本条工具消息之后挂新的 `loading` 占位，供工具结束后的续写。
pub(super) fn finalize_loading_assistant_before_tool_and_tail_with_new_loading(
    stream_ctx: &ChatStreamCallbackCtx,
    tool_message_id: &str,
) {
    let tool_present = with_active_session_ref(stream_ctx, |s| {
        s.messages.iter().any(|m| m.id == tool_message_id)
    })
    .unwrap_or(false);
    if !tool_present {
        return;
    }
    finalize_current_loading_streaming_assistant_row(stream_ctx);
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    with_active_session_mut(stream_ctx, |s| {
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
        stream_ctx.tail.replace_assistant_id(id);
        stream_ctx.tail.post_tool_stream_tail_cell().set(true);
    }
}

/// 同一轮 `run_agent_turn` 内可能多次调用模型（如外层 `continue 'outer` 规划改写），每次首段正文前都会再发
/// `assistant_answer_phase`。若仍写入同一 `assistant_message_id`，多段可见输出会挤在一个气泡里「不断刷新」。
/// 工具轮之间已有 [`finalize_loading_assistant_before_tool_and_tail_with_new_loading`]；此处补齐**无工具**的多轮。
pub(super) fn rotate_streaming_assistant_for_followup_model_round(
    stream_ctx: &ChatStreamCallbackCtx,
) {
    finalize_current_loading_streaming_assistant_row(stream_ctx);
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    with_active_session_mut(stream_ctx, |s| {
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
        stream_ctx.tail.replace_assistant_id(id);
        stream_ctx.tail.post_tool_stream_tail_cell().set(true);
    }
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 工具后续写段：分步/时间线等仍会 `push` 到列表末尾，需把当前 `loading` 占位再次移到最下方。
pub(super) fn ensure_streaming_assistant_tail_last(stream_ctx: &ChatStreamCallbackCtx) {
    if !stream_ctx.tail.post_tool_stream_tail_cell().get() {
        return;
    }
    let mid = stream_ctx.tail.clone_assistant_id();
    with_active_session_mut(stream_ctx, |s| {
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

pub(super) fn remove_loading_assistant_placeholder(stream_ctx: &ChatStreamCallbackCtx) {
    let mid = stream_ctx.tail.borrow_assistant_id();
    with_active_session_mut(stream_ctx, |s| {
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str())
            && s.messages[idx].role == "assistant"
            && s.messages[idx]
                .state
                .as_ref()
                .is_some_and(|st| st.is_loading())
        {
            s.messages.remove(idx);
        }
    });
}

/// 将内容追加到正在流式生成的 assistant 消息 text 中。
pub(super) fn append_to_assistant_text(
    aid: &str,
    mid: &str,
    chunk: &str,
    sessions: &RwSignal<Vec<ChatSession>>,
) {
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                m.text.push_str(chunk);
            }
        }
    });
}

/// `assistant_answer_phase` 之前的增量（思维链/思考区）：须写入 `reasoning_text`，`message_text_for_display_ex` 才能与终答 `text` 分流展示。
pub(super) fn append_to_assistant_reasoning(
    aid: &str,
    mid: &str,
    chunk: &str,
    sessions: &RwSignal<Vec<ChatSession>>,
) {
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                m.reasoning_text.push_str(chunk);
            }
        }
    });
}

pub(super) fn build_empty_reply_with_diagnostic(
    loc: Locale,
    answer_phase_entered: bool,
    answer_delta_chars: usize,
    stream_end_reason: Option<&str>,
) -> String {
    // 兜底保护：已有终答阶段且收到不少增量，但缺失 `stream_ended`，
    // 这更像“收尾中断”而非“无回复”，避免误导用户。
    let reason_unknown = stream_end_reason
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none_or(|s| s.eq_ignore_ascii_case("unknown"));
    if answer_phase_entered && answer_delta_chars > 0 && reason_unknown {
        let hint = i18n::stream_partial_finalize_missing_hint(loc);
        return format!(
            "{hint}\n\n{}",
            i18n::stream_empty_reply_diag_line(
                loc,
                stream_end_reason,
                answer_phase_entered,
                answer_delta_chars
            )
        );
    }
    let base = if !answer_phase_entered {
        i18n::stream_empty_reply_no_answer_phase(loc)
    } else if answer_delta_chars == 0 {
        i18n::stream_empty_reply_no_delta(loc)
    } else {
        i18n::stream_empty_reply(loc)
    };
    format!(
        "{base}\n\n{}",
        i18n::stream_empty_reply_diag_line(
            loc,
            stream_end_reason,
            answer_phase_entered,
            answer_delta_chars
        )
    )
}

pub(super) fn build_stream_error_with_suggestion(raw: &str, loc: Locale) -> String {
    let msg = raw.trim();
    if msg.is_empty() {
        return raw.to_string();
    }
    let low = msg.to_lowercase();
    let (impact, hint) = if low.contains("llm_api_key_required")
        || low.contains("api key")
        || low.contains("unauthorized")
        || low.contains("401")
    {
        (
            i18n::stream_err_impact_api_key(loc),
            i18n::stream_err_hint_api_key(loc),
        )
    } else if low.contains("timeout") || low.contains("timed out") || low.contains("408") {
        (
            i18n::stream_err_impact_timeout(loc),
            i18n::stream_err_hint_timeout(loc),
        )
    } else {
        (
            i18n::stream_err_impact_generic(loc),
            i18n::stream_err_hint_generic(loc),
        )
    };
    i18n::format_error_three_part(loc, msg, impact, hint)
}

pub(super) fn should_show_missing_final_summary_hint(
    end_reason: Option<&str>,
    in_answer_phase: bool,
    has_hierarchical_or_tool: bool,
    saw_final_response_timeline: bool,
) -> bool {
    // 须已收到 `assistant_answer_phase`：否则 `answer_delta_chars` 可能仅来自分层时间轴/子目标更新，
    // 与主气泡 `text` 无关，易误判「最终总结缺失」（见 issue：stream_ended=completed, answer_phase=false）。
    end_reason
        .and_then(|s| s.parse::<StreamEndReason>().ok())
        .is_some_and(|r| r == StreamEndReason::Completed)
        && in_answer_phase
        && has_hierarchical_or_tool
        && !saw_final_response_timeline
}
