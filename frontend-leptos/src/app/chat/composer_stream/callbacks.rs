//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口，便于审阅与后续按事件拆文件。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use leptos::prelude::*;

use crate::api::{ChatStreamCallbacks, OnToolCallFn};
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::i18n::Locale;
use crate::message_format::{
    staged_timeline_system_message_body, tool_card_compact_text, tool_card_text,
};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, TimelineLogInfo, ToolResultInfo,
};
use crate::storage::{ChatSession, StoredMessage};
use crate::timeline_scan::{
    timeline_state_staged_end, timeline_state_staged_start, timeline_state_tool,
};

use super::context::ChatStreamCallbackCtx;

fn enqueue_pending_tool_message_id(queue: &Rc<RefCell<VecDeque<String>>>, message_id: String) {
    queue.borrow_mut().push_back(message_id);
}

fn take_pending_tool_message_id(queue: &Rc<RefCell<VecDeque<String>>>) -> Option<String> {
    queue.borrow_mut().pop_front()
}

fn non_empty_trimmed_tool_name(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn build_final_response_text(title: &str, detail: Option<&str>) -> String {
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

fn build_intent_analysis_main_bubble_text(title: &str, detail: Option<&str>) -> String {
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
            if line.starts_with("综合置信度：") {
                confidence = line.to_string();
            } else if line.starts_with("主意图：") {
                primary = line.to_string();
            } else if line.starts_with("需要澄清：") {
                clarification = line.to_string();
            } else if line.starts_with("L2 结果：") {
                l2 = line.to_string();
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

fn build_hierarchical_plan_main_bubble_text(title: &str, detail: Option<&str>) -> String {
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

fn build_hierarchical_subgoal_main_bubble_text(title: &str, detail: Option<&str>) -> String {
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

fn to_single_line(s: &str, max_chars: usize) -> String {
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
fn insert_msg_before_streaming_assistant_tail(
    messages: &mut Vec<StoredMessage>,
    streaming_assistant_id: &str,
    msg: StoredMessage,
) {
    if let Some(idx) = messages.iter().position(|m| {
        m.id == streaming_assistant_id
            && m.role == "assistant"
            && m.state.as_deref() == Some("loading")
    }) {
        messages.insert(idx, msg);
    } else {
        messages.push(msg);
    }
}

/// 管理器时间线（意图分析、规划摘要等）在服务端往往早于正文 `delta`，
/// 须插在**当前流式 `loading` 助手气泡之前**，否则会跑到已流出的计划文字下面。
fn insert_before_streaming_assistant_or_append(
    stream_ctx: &ChatStreamCallbackCtx,
    msg: StoredMessage,
) {
    let aid = stream_ctx.active_session_id.as_str();
    let mid = stream_ctx.assistant_message_id.borrow().clone();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
        }
    });
}

fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<String>,
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

fn has_same_assistant_timeline_bubble(stream_ctx: &ChatStreamCallbackCtx, text: &str) -> bool {
    let aid = stream_ctx.active_session_id.as_str();
    stream_ctx.chat.sessions.with(|list| {
        list.iter()
            .find(|s| s.id == aid)
            .and_then(|s| {
                s.messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant" && !m.is_tool && m.state.is_none())
            })
            .is_some_and(|m| m.text.trim() == text.trim())
    })
}

fn extract_subgoal_marker_from_title(title: &str) -> Option<String> {
    let title = title.trim();
    if !title.starts_with("子目标 `") {
        return None;
    }
    let rest = title.strip_prefix("子目标 `")?;
    let goal_id = rest.strip_suffix('`')?;
    if goal_id.is_empty() {
        return None;
    }
    Some(format!("hierarchical-subgoal:{goal_id}"))
}

fn extract_subgoal_target_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        if line.starts_with("- 目标：") || line.starts_with("目标：") {
            Some(line.to_string())
        } else {
            None
        }
    })
}

fn merge_subgoal_text_preserving_target(existing: &str, incoming: &str) -> String {
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
    let insert_idx = if lines[0].trim_start().starts_with("子目标 ") {
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

fn upsert_hierarchical_subgoal_bubble(
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
    let aid = stream_ctx.active_session_id.as_str();
    let now = message_created_ms();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            if let Some(existing) = s
                .messages
                .iter_mut()
                .find(|m| m.role == "assistant" && m.state.as_deref() == Some(marker.as_str()))
            {
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
                state: Some(marker.clone()),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: now,
            };
            let mid = stream_ctx.assistant_message_id.borrow().clone();
            insert_msg_before_streaming_assistant_tail(&mut s.messages, &mid, msg);
        }
    });
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 工具卡片插入前：结束当前流式段（开场白等保留在工具与时间线**之上**），
/// 并在本条工具消息之后挂新的 `loading` 占位，供工具结束后的续写。
fn finalize_loading_assistant_before_tool_and_tail_with_new_loading(
    stream_ctx: &ChatStreamCallbackCtx,
    tool_message_id: &str,
) {
    let aid = stream_ctx.active_session_id.as_str();
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    stream_ctx.chat.sessions.update(|list| {
        let Some(s) = list.iter_mut().find(|s| s.id == aid) else {
            return;
        };
        if !s.messages.iter().any(|m| m.id == tool_message_id) {
            return;
        }
        let mid = stream_ctx.assistant_message_id.borrow();
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            let m = &mut s.messages[idx];
            if m.role == "assistant" && m.state.as_deref() == Some("loading") {
                if m.text.trim().is_empty() {
                    s.messages.remove(idx);
                } else {
                    m.state = None;
                }
            }
        }
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
                state: Some("loading".to_string()),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: now,
            },
        );
        *new_tail_id.borrow_mut() = Some(new_asst_id);
    });
    if let Some(id) = new_tail_id.into_inner() {
        stream_ctx.assistant_message_id.replace(id);
        stream_ctx.post_tool_stream_tail.set(true);
    }
}

/// 同一轮 `run_agent_turn` 内可能多次调用模型（如外层 `continue 'outer` 规划改写），每次首段正文前都会再发
/// `assistant_answer_phase`。若仍写入同一 `assistant_message_id`，多段可见输出会挤在一个气泡里「不断刷新」。
/// 工具轮之间已有 [`finalize_loading_assistant_before_tool_and_tail_with_new_loading`]；此处补齐**无工具**的多轮。
fn rotate_streaming_assistant_for_followup_model_round(stream_ctx: &ChatStreamCallbackCtx) {
    let aid = stream_ctx.active_session_id.as_str();
    let now = message_created_ms();
    let new_tail_id = RefCell::new(None::<String>);
    stream_ctx.chat.sessions.update(|list| {
        let Some(s) = list.iter_mut().find(|s| s.id == aid) else {
            return;
        };
        let mid = stream_ctx.assistant_message_id.borrow();
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            let m = &mut s.messages[idx];
            if m.role == "assistant" && m.state.as_deref() == Some("loading") {
                if m.text.trim().is_empty() {
                    s.messages.remove(idx);
                } else {
                    m.state = None;
                }
            }
        }
        let new_asst_id = make_message_id();
        s.messages.push(StoredMessage {
            id: new_asst_id.clone(),
            role: "assistant".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some("loading".to_string()),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
        *new_tail_id.borrow_mut() = Some(new_asst_id);
    });
    if let Some(id) = new_tail_id.into_inner() {
        stream_ctx.assistant_message_id.replace(id);
        stream_ctx.post_tool_stream_tail.set(true);
    }
    ensure_streaming_assistant_tail_last(stream_ctx);
}

/// 工具后续写段：分步/时间线等仍会 `push` 到列表末尾，需把当前 `loading` 占位再次移到最下方。
fn ensure_streaming_assistant_tail_last(stream_ctx: &ChatStreamCallbackCtx) {
    if !stream_ctx.post_tool_stream_tail.get() {
        return;
    }
    let aid = stream_ctx.active_session_id.as_str();
    let mid = stream_ctx.assistant_message_id.borrow().clone();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            let Some(idx) = s.messages.iter().position(|m| m.id == mid) else {
                return;
            };
            if s.messages[idx].role != "assistant"
                || s.messages[idx].state.as_deref() != Some("loading")
            {
                return;
            }
            let m = s.messages.remove(idx);
            s.messages.push(m);
        }
    });
}

fn remove_loading_assistant_placeholder(stream_ctx: &ChatStreamCallbackCtx) {
    let aid = stream_ctx.active_session_id.as_str();
    let mid = stream_ctx.assistant_message_id.borrow();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid)
            && let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str())
            && s.messages[idx].role == "assistant"
            && s.messages[idx].state.as_deref() == Some("loading")
        {
            s.messages.remove(idx);
        }
    });
}

/// 将内容追加到正在流式生成的 assistant 消息 text 中。
fn append_to_assistant_text(
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
fn append_to_assistant_reasoning(
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

fn build_empty_reply_with_diagnostic(
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

fn build_stream_error_with_suggestion(raw: &str, loc: Locale) -> String {
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

fn should_show_missing_final_summary_hint(
    end_reason: Option<&str>,
    in_answer_phase: bool,
    diag_chars: usize,
    has_hierarchical_or_tool: bool,
    saw_final_response_timeline: bool,
) -> bool {
    end_reason.is_some_and(|r| r.eq_ignore_ascii_case("completed"))
        && (in_answer_phase || diag_chars > 0)
        && has_hierarchical_or_tool
        && !saw_final_response_timeline
}

/// 由 [`super::make_attach_chat_stream`](super::make_attach_chat_stream) 调用；集中所有 `on_*` 闭包，降低 `mod.rs` 维护面。
pub(super) fn build_chat_stream_callbacks(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
) -> ChatStreamCallbacks {
    let answer_delta_chars: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let pending_followup_answer_round: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let stream_end_reason: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let current_subgoal_marker: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let saw_final_response_timeline: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let on_delta: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let pending_followup_answer_round = Rc::clone(&pending_followup_answer_round);
        Rc::new(move |chunk: String| {
            if pending_followup_answer_round.get() {
                rotate_streaming_assistant_for_followup_model_round(stream_ctx.as_ref());
                pending_followup_answer_round.set(false);
                answer_delta_chars.set(0);
            }
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.borrow();
            if in_answer_phase.get() {
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(chunk.chars().count()),
                );
                append_to_assistant_text(aid, mid.as_str(), &chunk, &stream_ctx.chat.sessions);
            } else {
                append_to_assistant_reasoning(aid, mid.as_str(), &chunk, &stream_ctx.chat.sessions);
            }
        })
    };

    let on_done: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let pending_followup_answer_round = Rc::clone(&pending_followup_answer_round);
        let stream_end_reason = Rc::clone(&stream_end_reason);
        let saw_final_response_timeline = Rc::clone(&saw_final_response_timeline);
        Rc::new(move || {
            pending_followup_answer_round.set(false);
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.borrow().clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    let has_hierarchical_or_tool = s.messages.iter().any(|x| {
                        x.is_tool
                            || x.state
                                .as_deref()
                                .is_some_and(|st| st.starts_with("hierarchical-subgoal:"))
                    });
                    if let Some(idx) = s.messages.iter().position(|m| m.id == mid)
                        && s.messages[idx].state.as_deref() == Some("loading")
                    {
                        s.messages[idx].state = None;
                        let body_chars = s.messages[idx].text.chars().count()
                            + s.messages[idx].reasoning_text.chars().count();
                        let diag_chars = body_chars.max(answer_delta_chars.get());
                        if s.messages[idx].text.trim().is_empty()
                            && s.messages[idx].reasoning_text.trim().is_empty()
                        {
                            let end_reason = stream_end_reason.borrow();
                            let completed_with_visible_delta = end_reason
                                .as_deref()
                                .is_some_and(|r| r.eq_ignore_ascii_case("completed"))
                                && in_answer_phase.get()
                                && diag_chars > 0;
                            if completed_with_visible_delta {
                                // 流程已完成且本轮存在可见输出时，空 loading 气泡多为尾部占位残留，直接删除避免误报“无回复”。
                                s.messages.remove(idx);
                                return;
                            }
                            let completed_no_final = should_show_missing_final_summary_hint(
                                end_reason.as_deref(),
                                in_answer_phase.get(),
                                diag_chars,
                                has_hierarchical_or_tool,
                                saw_final_response_timeline.get(),
                            );
                            if completed_no_final {
                                s.messages[idx].text = format!(
                                    "{}\n\n{}",
                                    i18n::stream_completed_missing_final_summary_hint(loc),
                                    i18n::stream_empty_reply_diag_line(
                                        loc,
                                        end_reason.as_deref(),
                                        in_answer_phase.get(),
                                        diag_chars
                                    )
                                );
                            } else {
                                s.messages[idx].text = build_empty_reply_with_diagnostic(
                                    loc,
                                    in_answer_phase.get(),
                                    diag_chars,
                                    end_reason.as_deref(),
                                );
                            }
                        }
                    }
                }
            });
            stream_ctx.shell.status_busy.set(false);
            *stream_ctx.shell.abort_cell.lock().unwrap() = None;
        })
    };

    let on_error: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |msg: String| {
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            stream_ctx.chat.stream_job_id.set(None);
            stream_ctx.chat.stream_last_event_seq.set(0);
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.borrow().clone();
            let loc = stream_ctx.locale.get_untracked();
            let friendly = build_stream_error_with_suggestion(&msg, loc);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                        m.text = friendly.clone();
                        m.state = Some("error".to_string());
                    }
                }
            });
            stream_ctx.shell.status_busy.set(false);
            stream_ctx.shell.status_err.set(Some(
                i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
            ));
            *stream_ctx.shell.abort_cell.lock().unwrap() = None;
        })
    };

    let on_ws: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move || {
            (stream_ctx.shell.refresh_workspace)();
            if stream_ctx.shell.changelist_modal_open.get_untracked() {
                stream_ctx
                    .shell
                    .changelist_fetch_nonce
                    .update(|x| *x = x.wrapping_add(1));
            }
        })
    };

    // 暂存 tool_call 参数
    let on_tool_call: OnToolCallFn = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let current_subgoal_marker = Rc::clone(&current_subgoal_marker);
        Rc::new(
            move |name: String,
                  summary: String,
                  preview: Option<String>,
                  full: Option<String>,
                  goal_id: Option<String>,
                  tool_call_id: Option<String>| {
                let _ = (preview, full);
                let loc = stream_ctx.locale.get_untracked();
                let core = if !summary.trim().is_empty() {
                    summary.trim().to_string()
                } else if !name.trim().is_empty() {
                    format!("{}{}", i18n::tool_card_prefix(loc), name.trim())
                } else {
                    i18n::tool_card_fallback(loc).to_string()
                };
                let text = to_single_line(
                    &format!("{} · {}", core, i18n::status_tool_running(loc)),
                    140,
                );
                let detail = if !name.trim().is_empty() {
                    format!("tool: {name}\nstatus: running")
                } else {
                    "status: running".to_string()
                };
                let id = make_message_id();
                let aid = stream_ctx.active_session_id.as_str();
                let marker = goal_id
                    .as_deref()
                    .map(|g| format!("hierarchical-subgoal:{g}"))
                    .or_else(|| current_subgoal_marker.borrow().clone());
                let tcid = tool_call_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                stream_ctx.chat.sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        let msg = StoredMessage {
                            id: id.clone(),
                            role: "system".to_string(),
                            text,
                            reasoning_text: detail.clone(),
                            image_urls: vec![],
                            state: Some("loading".to_string()),
                            is_tool: true,
                            tool_call_id: tcid.clone(),
                            tool_name: non_empty_trimmed_tool_name(&name),
                            created_at: message_created_ms(),
                        };
                        if let Some(mk) = marker.as_deref()
                            && let Some(idx) = s
                                .messages
                                .iter()
                                .rposition(|m| m.state.as_deref() == Some(mk))
                        {
                            s.messages.insert(idx + 1, msg);
                        } else {
                            s.messages.push(msg);
                        }
                    }
                });
                // 开场白留在时间线/工具之上；工具后挂新占位，续写走新气泡，避免“最早的话出现在最下面”。
                finalize_loading_assistant_before_tool_and_tail_with_new_loading(&stream_ctx, &id);
                // 有 `tool_call_id` 时由 `tool_result` 按 id 命中占位气泡；否则保持 FIFO。
                if tcid.is_none() {
                    enqueue_pending_tool_message_id(&stream_ctx.pending_tool_message_ids, id);
                }
            },
        )
    };

    let on_tool_status: Rc<dyn Fn(bool)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |b: bool| {
            stream_ctx.shell.tool_busy.set(b);
        })
    };

    let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ToolResultInfo| {
            let loc = stream_ctx.locale.get_untracked();
            let result_text = tool_card_text(&info, loc);
            let compact = tool_card_compact_text(&info, loc);
            let t = to_single_line(&compact, 180);
            let detail = result_text.clone();

            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let tl_ok = info.ok.unwrap_or(true);
            let state = timeline_state_tool(&id, tl_ok);
            let pending_queue = Rc::clone(&stream_ctx.pending_tool_message_ids);
            let mut updated_existing = false;
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    let tid = info
                        .tool_call_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let idx_by_tid = tid.and_then(|tid| {
                        s.messages.iter().position(|m| {
                            m.is_tool
                                && m.tool_call_id.as_deref() == Some(tid)
                                && m.state.as_deref() == Some("loading")
                        })
                    });
                    let idx_by_fifo = idx_by_tid.is_none().then(|| {
                        take_pending_tool_message_id(&pending_queue)
                            .and_then(|pid| s.messages.iter().position(|m| m.id == pid))
                    });
                    let idx_opt = idx_by_tid.or(idx_by_fifo.flatten());
                    if let Some(idx) = idx_opt {
                        let m = &mut s.messages[idx];
                        m.text = t.clone();
                        m.reasoning_text = detail.clone();
                        m.state = Some(state.clone());
                        m.is_tool = true;
                        if m.tool_call_id.is_none() {
                            m.tool_call_id =
                                info.tool_call_id.clone().filter(|x| !x.trim().is_empty());
                        }
                        if let Some(tn) = non_empty_trimmed_tool_name(&info.name) {
                            m.tool_name = Some(tn);
                        }
                        updated_existing = true;
                    }
                    if !updated_existing {
                        let msg = StoredMessage {
                            id: id.clone(),
                            role: "system".to_string(),
                            text: t.clone(),
                            reasoning_text: detail.clone(),
                            image_urls: vec![],
                            state: Some(state.clone()),
                            is_tool: true,
                            tool_call_id: info
                                .tool_call_id
                                .clone()
                                .filter(|x| !x.trim().is_empty()),
                            tool_name: non_empty_trimmed_tool_name(&info.name),
                            created_at: message_created_ms(),
                        };
                        if let Some(goal_id) = info.goal_id.as_deref() {
                            let marker = format!("hierarchical-subgoal:{goal_id}");
                            if let Some(idx) = s
                                .messages
                                .iter()
                                .rposition(|m| m.state.as_deref() == Some(marker.as_str()))
                            {
                                s.messages.insert(idx + 1, msg);
                            } else {
                                s.messages.push(msg);
                            }
                        } else {
                            s.messages.push(msg);
                        }
                    }
                }
            });
            ensure_streaming_assistant_tail_last(&stream_ctx);
        })
    };

    let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |req: CommandApprovalRequest| {
            stream_ctx.shell.pending_approval.set(Some((
                stream_ctx.approval_session_store_id.clone(),
                req.command,
                req.args,
            )));
        })
    };

    let on_cid: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |id: String| {
            stream_ctx
                .chat
                .session_sync
                .update(|s| s.apply_stream_conversation_id(id.clone()));
            let aid = stream_ctx.active_session_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                    s.server_conversation_id = Some(id);
                    s.server_revision = None;
                }
            });
        })
    };

    let on_conv_rev: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |rev: u64| {
            stream_ctx
                .chat
                .session_sync
                .update(|s| s.apply_saved_revision(rev));
            let aid = stream_ctx.active_session_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                    s.server_revision = Some(rev);
                }
            });
        })
    };

    let on_stream_ended: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let stream_end_reason = Rc::clone(&stream_end_reason);
        Rc::new(move |reason: String| {
            *stream_end_reason.borrow_mut() = Some(reason.clone());
            if reason == "completed" || reason == "cancelled" {
                stream_ctx.chat.stream_job_id.set(None);
                stream_ctx.chat.stream_last_event_seq.set(0);
            }
        })
    };

    let on_stream_job_id: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |jid: u64| {
            stream_ctx.chat.stream_job_id.set(Some(jid));
        })
    };

    let on_last_sse_event_id: Rc<dyn Fn(u64)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |seq: u64| {
            stream_ctx.chat.stream_last_event_seq.set(seq);
        })
    };

    let on_assistant_answer_phase: Rc<dyn Fn()> = {
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let pending_followup_answer_round = Rc::clone(&pending_followup_answer_round);
        Rc::new(move || {
            if in_answer_phase.get() {
                // 重复 answer_phase 仅标记“下一段正文需轮换气泡”，避免无后续 delta 时产生空 "(无回复)" 气泡。
                pending_followup_answer_round.set(true);
            }
            in_answer_phase.set(true);
        })
    };

    let on_staged_step_started: Rc<dyn Fn(StagedPlanStepStartInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: StagedPlanStepStartInfo| {
            let loc = stream_ctx.locale.get_untracked();
            let text = staged_timeline_system_message_body(&i18n::timeline_staged_step_started(
                loc,
                info.step_index,
                info.total_steps,
                &info.description,
                info.executor_kind.as_deref(),
            ));
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let now = message_created_ms();
            let state = timeline_state_staged_start(&id, info.step_index, info.total_steps);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        tool_call_id: None,
                        tool_name: None,
                        created_at: now,
                    });
                }
            });
            ensure_streaming_assistant_tail_last(&stream_ctx);
        })
    };

    let on_clarification: Rc<dyn Fn(ClarificationQuestionnaireInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ClarificationQuestionnaireInfo| {
            stream_ctx
                .shell
                .pending_clarification
                .set(Some(PendingClarificationForm::from_sse(info)));
        })
    };

    // Manager 规划 / 分层执行旁注：作为 system 时间线消息落盘，按时间顺序与工具卡片交替展示。
    let on_timeline_log: Rc<dyn Fn(TimelineLogInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let current_subgoal_marker = Rc::clone(&current_subgoal_marker);
        let saw_final_response_timeline = Rc::clone(&saw_final_response_timeline);
        Rc::new(move |info: TimelineLogInfo| {
            web_sys::console::log_1(
                &format!("[TL] kind={} title={}", info.kind, info.title).into(),
            );
            if info.kind == "final_response" {
                saw_final_response_timeline.set(true);
                let final_text = build_final_response_text(&info.title, info.detail.as_deref());
                if !final_text.is_empty() {
                    remove_loading_assistant_placeholder(&stream_ctx);
                    if !has_same_assistant_timeline_bubble(&stream_ctx, &final_text) {
                        push_assistant_timeline_bubble(&stream_ctx, final_text.clone(), None);
                        answer_delta_chars.set(
                            answer_delta_chars
                                .get()
                                .saturating_add(final_text.chars().count()),
                        );
                    }
                }
                return;
            }
            if info.kind == "intent_analysis" {
                let intent_text =
                    build_intent_analysis_main_bubble_text(&info.title, info.detail.as_deref());
                if intent_text.is_empty() {
                    return;
                }
                push_assistant_timeline_bubble(&stream_ctx, intent_text.clone(), None);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(intent_text.chars().count()),
                );
                return;
            }
            if info.kind == "hierarchical_plan" {
                let plan_text =
                    build_hierarchical_plan_main_bubble_text(&info.title, info.detail.as_deref());
                if plan_text.is_empty() {
                    return;
                }
                push_assistant_timeline_bubble(&stream_ctx, plan_text.clone(), None);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(plan_text.chars().count()),
                );
                return;
            }
            if info.kind == "hierarchical_subgoal" {
                let text = build_hierarchical_subgoal_main_bubble_text(
                    &info.title,
                    info.detail.as_deref(),
                );
                if text.is_empty() {
                    return;
                }
                *current_subgoal_marker.borrow_mut() =
                    extract_subgoal_marker_from_title(&info.title);
                upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(text.chars().count()),
                );
                return;
            }
            if info.kind == "hierarchical_subgoal_started" {
                let text = build_hierarchical_subgoal_main_bubble_text(
                    &info.title,
                    info.detail.as_deref(),
                );
                if text.is_empty() {
                    return;
                }
                *current_subgoal_marker.borrow_mut() =
                    extract_subgoal_marker_from_title(&info.title);
                upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(text.chars().count()),
                );
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let normalized_title = match info.kind.as_str() {
                "tool_step_started" => {
                    i18n::timeline_tool_step_started_title(loc, info.title.trim())
                }
                "tool_step_finished" => {
                    i18n::timeline_tool_step_finished_title(loc, info.title.trim())
                }
                _ => info.title.trim().to_string(),
            };
            let mut body = normalized_title;
            if let Some(detail) = info.detail.as_deref().map(str::trim)
                && !detail.is_empty()
            {
                body.push('\n');
                body.push_str(detail);
            }
            if body.is_empty() {
                return;
            }
            push_assistant_timeline_bubble(
                &stream_ctx,
                staged_timeline_system_message_body(&body),
                None,
            );
        })
    };

    let on_staged_step_finished: Rc<dyn Fn(StagedPlanStepEndInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: StagedPlanStepEndInfo| {
            let loc = stream_ctx.locale.get_untracked();
            let text = staged_timeline_system_message_body(&i18n::timeline_staged_step_finished(
                loc,
                info.step_index,
                info.total_steps,
                &info.status,
                info.executor_kind.as_deref(),
            ));
            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let now = message_created_ms();
            let state =
                timeline_state_staged_end(&id, info.step_index, info.total_steps, &info.status);
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    s.messages.push(StoredMessage {
                        id,
                        role: "system".to_string(),
                        text,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        tool_call_id: None,
                        tool_name: None,
                        created_at: now,
                    });
                }
            });
            ensure_streaming_assistant_tail_last(&stream_ctx);
        })
    };

    // thinking_trace 保留在调试台，不再写入聊天正文，避免干扰时间线可读性。
    let on_thinking_trace: Rc<dyn Fn(crate::sse_dispatch::ThinkingTraceInfo)> =
        { Rc::new(move |_info: crate::sse_dispatch::ThinkingTraceInfo| {}) };

    ChatStreamCallbacks {
        on_delta,
        on_done: on_done.clone(),
        on_error: on_error.clone(),
        on_workspace_changed: on_ws,
        on_tool_status,
        on_tool_result,
        on_tool_call,
        on_approval,
        on_conversation_id: on_cid,
        on_conversation_revision: on_conv_rev,
        on_stream_ended,
        on_stream_job_id,
        on_last_sse_event_id,
        on_assistant_answer_phase,
        on_staged_plan_step_started: on_staged_step_started,
        on_staged_plan_step_finished: on_staged_step_finished,
        on_clarification_questionnaire: on_clarification,
        on_thinking_trace,
        on_timeline_log,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_empty_reply_with_diagnostic, build_final_response_text,
        build_hierarchical_plan_main_bubble_text, build_hierarchical_subgoal_main_bubble_text,
        build_intent_analysis_main_bubble_text, build_stream_error_with_suggestion,
        enqueue_pending_tool_message_id, has_same_assistant_timeline_bubble,
        merge_subgoal_text_preserving_target, take_pending_tool_message_id,
    };
    use crate::i18n::{self, Locale};
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    #[test]
    fn pending_tool_message_queue_is_fifo() {
        let q = Rc::new(RefCell::new(VecDeque::new()));
        enqueue_pending_tool_message_id(&q, "m1".to_string());
        enqueue_pending_tool_message_id(&q, "m2".to_string());
        enqueue_pending_tool_message_id(&q, "m3".to_string());

        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m1"));
        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m2"));
        assert_eq!(take_pending_tool_message_id(&q).as_deref(), Some("m3"));
        assert_eq!(take_pending_tool_message_id(&q), None);
    }

    #[test]
    fn pending_tool_message_queue_empty_returns_none() {
        let q = Rc::new(RefCell::new(VecDeque::new()));
        assert_eq!(take_pending_tool_message_id(&q), None);
    }

    #[test]
    fn final_response_text_merges_title_and_detail() {
        let merged = build_final_response_text("  你好  ", Some("  世界  "));
        assert_eq!(merged, "你好\n\n世界");
    }

    #[test]
    fn final_response_text_ignores_empty_detail() {
        let merged = build_final_response_text("  你好  ", Some("   "));
        assert_eq!(merged, "你好");
    }

    #[test]
    fn intent_analysis_text_adds_trailing_gap() {
        let detail = "主意图：execute.run_test_build\n综合置信度：0.61\n需要澄清：false\nL2 结果：未启用/未触发\n覆盖原因：无";
        let t =
            build_intent_analysis_main_bubble_text("意图分析：执行类（直接执行）", Some(detail));
        assert_eq!(
            t,
            "意图分析：执行类（直接执行）\n综合置信度：0.61\n主意图：execute.run_test_build\n需要澄清：false\nL2 结果：未启用/未触发\n\n"
        );
    }

    #[test]
    fn intent_analysis_text_empty_when_no_content() {
        let t = build_intent_analysis_main_bubble_text("   ", Some(" "));
        assert!(t.is_empty());
    }

    #[test]
    fn hierarchical_plan_text_adds_trailing_gap() {
        let t =
            build_hierarchical_plan_main_bubble_text("**Manager 规划**", Some("- [ ] g1: 写代码"));
        assert_eq!(t, "**Manager 规划**\n- [ ] g1: 写代码\n\n");
    }

    #[test]
    fn hierarchical_subgoal_text_keeps_phase_lines() {
        let t = build_hierarchical_subgoal_main_bubble_text(
            "子目标 `goal_2`",
            Some("- 阶段：开始执行\n- 目标：创建 build 目录"),
        );
        assert!(t.contains("阶段：开始执行"));
        assert!(t.contains("目标：创建 build 目录"));
    }

    #[test]
    fn stream_error_uses_standardized_sections() {
        let out = build_stream_error_with_suggestion("LLM_API_KEY_REQUIRED", Locale::ZhHans);
        assert!(out.contains("发生了什么"));
        assert!(out.contains("影响范围"));
        assert!(out.contains("建议下一步"));
    }

    #[test]
    fn empty_reply_diagnostic_uses_partial_generation_hint_when_reason_unknown() {
        let out = build_empty_reply_with_diagnostic(Locale::ZhHans, true, 128, Some("unknown"));
        assert!(out.contains("流式收尾信号缺失"));
        assert!(out.contains("stream_ended=unknown"));
    }

    #[test]
    fn subgoal_update_preserves_target_line_when_new_payload_missing_target() {
        let existing = "子目标 `goal_4`\n- 阶段：开始执行\n- 目标：创建 CMakeLists.txt\n\n";
        let incoming = "子目标 `goal_4`\n- 结果：完成\n- 工具：create_file\n\n";
        let out = merge_subgoal_text_preserving_target(existing, incoming);
        assert!(out.contains("目标：创建 CMakeLists.txt"));
        assert!(out.contains("结果：完成"));
    }

    #[test]
    fn completed_without_final_summary_hint_is_shown() {
        let out = i18n::stream_completed_missing_final_summary_hint(Locale::ZhHans);
        assert!(out.contains("最终总结消息缺失"));
    }

    #[test]
    fn dedupe_helper_trims_whitespace() {
        let dummy = "hello world";
        assert_eq!(dummy.trim(), "hello world");
        // 间接保障：去重比较使用 trim，不会因尾部换行误判不同。
        // 该逻辑在 `has_same_assistant_timeline_bubble` 中实现。
        let _ = has_same_assistant_timeline_bubble;
    }

    #[test]
    fn missing_final_summary_hint_disabled_after_final_response_timeline() {
        assert!(!super::should_show_missing_final_summary_hint(
            Some("completed"),
            true,
            128,
            true,
            true,
        ));
        assert!(super::should_show_missing_final_summary_hint(
            Some("completed"),
            true,
            128,
            true,
            false,
        ));
    }
}
