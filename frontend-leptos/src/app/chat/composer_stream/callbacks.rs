//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口，便于审阅与后续按事件拆文件。

use std::cell::Cell;
use std::rc::Rc;
use std::{cell::RefCell, collections::VecDeque};

use leptos::prelude::*;

use crate::api::ChatStreamCallbacks;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::i18n::Locale;
use crate::message_format::{staged_timeline_system_message_body, tool_card_text};
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

fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<String>,
) {
    if text.trim().is_empty() {
        return;
    }
    let id = make_message_id();
    let aid = stream_ctx.active_session_id.as_str();
    let now = message_created_ms();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            s.messages.push(StoredMessage {
                id,
                role: "assistant".to_string(),
                text,
                reasoning_text: String::new(),
                image_urls: vec![],
                state,
                is_tool: false,
                created_at: now,
            });
        }
    });
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
                existing.text = text.clone();
                existing.created_at = now;
                return;
            }
            s.messages.push(StoredMessage {
                id: make_message_id(),
                role: "assistant".to_string(),
                text: text.clone(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(marker.clone()),
                is_tool: false,
                created_at: now,
            });
        }
    });
}

fn move_loading_assistant_to_bottom(stream_ctx: &ChatStreamCallbackCtx) {
    let aid = stream_ctx.active_session_id.as_str();
    let mid = stream_ctx.assistant_message_id.as_str();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid)
            && let Some(idx) = s.messages.iter().position(|m| m.id == mid)
            && s.messages[idx].role == "assistant"
            && s.messages[idx].state.as_deref() == Some("loading")
        {
            let m = s.messages.remove(idx);
            s.messages.push(m);
        }
    });
}

fn remove_loading_assistant_placeholder(stream_ctx: &ChatStreamCallbackCtx) {
    let aid = stream_ctx.active_session_id.as_str();
    let mid = stream_ctx.assistant_message_id.as_str();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid)
            && let Some(idx) = s.messages.iter().position(|m| m.id == mid)
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

fn build_empty_reply_with_diagnostic(
    loc: Locale,
    answer_phase_entered: bool,
    answer_delta_chars: usize,
    stream_end_reason: Option<&str>,
) -> String {
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

/// 由 [`super::make_attach_chat_stream`](super::make_attach_chat_stream) 调用；集中所有 `on_*` 闭包，降低 `mod.rs` 维护面。
pub(super) fn build_chat_stream_callbacks(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
) -> ChatStreamCallbacks {
    let answer_delta_chars: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let stream_end_reason: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let current_subgoal_marker: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    // 兜底缓冲：若服务端未下发 assistant_answer_phase，但确实有 delta，
    // 则在 on_done 且正文仍为空时回填，避免出现“后端有答复、前端无回复”。
    let pre_answer_buffer: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let on_delta: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let pre_answer_buffer = Rc::clone(&pre_answer_buffer);
        Rc::new(move |chunk: String| {
            let aid = stream_ctx.active_session_id.as_str();
            let mid = stream_ctx.assistant_message_id.as_str();
            if in_answer_phase.get() {
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(chunk.chars().count()),
                );
                append_to_assistant_text(aid, mid, &chunk, &stream_ctx.chat.sessions);
            } else {
                pre_answer_buffer.borrow_mut().push_str(&chunk);
            }
        })
    };

    let on_done: Rc<dyn Fn()> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let in_answer_phase = Rc::clone(&in_answer_phase);
        let answer_delta_chars = Rc::clone(&answer_delta_chars);
        let stream_end_reason = Rc::clone(&stream_end_reason);
        let pre_answer_buffer = Rc::clone(&pre_answer_buffer);
        Rc::new(move || {
            if *stream_ctx.shell.user_cancelled_stream.lock().unwrap() {
                *stream_ctx.shell.abort_cell.lock().unwrap() = None;
                return;
            }
            let loc = stream_ctx.locale.get_untracked();
            let aid = stream_ctx.active_session_id.clone();
            let mid = stream_ctx.assistant_message_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                        && m.state.as_deref() == Some("loading")
                    {
                        m.state = None;
                        if m.text.trim().is_empty() {
                            let buffered = pre_answer_buffer.borrow();
                            if !buffered.trim().is_empty() {
                                m.text = buffered.clone();
                                answer_delta_chars.set(
                                    answer_delta_chars
                                        .get()
                                        .saturating_add(m.text.chars().count()),
                                );
                                web_sys::console::log_1(
                                    &"[SSE] fallback_from_pre_answer_buffer=true".into(),
                                );
                            }
                        }
                        if m.text.trim().is_empty() {
                            let end_reason = stream_end_reason.borrow();
                            m.text = build_empty_reply_with_diagnostic(
                                loc,
                                in_answer_phase.get(),
                                answer_delta_chars.get(),
                                end_reason.as_deref(),
                            );
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
            let mid = stream_ctx.assistant_message_id.clone();
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                        m.text = msg;
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
    let on_tool_call: Rc<dyn Fn(String, String, Option<String>, Option<String>, Option<String>)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let current_subgoal_marker = Rc::clone(&current_subgoal_marker);
        Rc::new(
            move |name: String,
                  summary: String,
                  preview: Option<String>,
                  full: Option<String>,
                  goal_id: Option<String>| {
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
                // 工具调用出现后，保持“助手正在生成”气泡在最底部，避免视觉时序倒置。
                move_loading_assistant_to_bottom(&stream_ctx);
                enqueue_pending_tool_message_id(&stream_ctx.pending_tool_message_ids, id);
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
            let result_text = tool_card_text(&info, stream_ctx.locale.get_untracked());
            let t = to_single_line(&result_text, 180);
            let detail = result_text.clone();

            let id = make_message_id();
            let aid = stream_ctx.active_session_id.as_str();
            let tl_ok = info.ok.unwrap_or(true);
            let state = timeline_state_tool(&id, tl_ok);
            let pending_id = take_pending_tool_message_id(&stream_ctx.pending_tool_message_ids);
            let mut updated_existing = false;
            stream_ctx.chat.sessions.update(|list| {
                if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                    if let Some(pid) = pending_id.as_deref()
                        && let Some(m) = s.messages.iter_mut().find(|m| m.id == pid)
                    {
                        m.text = t.clone();
                        m.reasoning_text = detail.clone();
                        m.state = Some(state.clone());
                        m.is_tool = true;
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
            // 工具结果落盘后，同样把 loading 助手气泡放到底部。
            move_loading_assistant_to_bottom(&stream_ctx);
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
        Rc::new(move || in_answer_phase.set(true))
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
                        created_at: now,
                    });
                }
            });
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
        Rc::new(move |info: TimelineLogInfo| {
            web_sys::console::log_1(
                &format!("[TL] kind={} title={}", info.kind, info.title).into(),
            );
            if info.kind == "final_response" {
                let final_text = build_final_response_text(&info.title, info.detail.as_deref());
                if !final_text.is_empty() {
                    remove_loading_assistant_placeholder(&stream_ctx);
                    push_assistant_timeline_bubble(&stream_ctx, final_text.clone(), None);
                    answer_delta_chars.set(
                        answer_delta_chars
                            .get()
                            .saturating_add(final_text.chars().count()),
                    );
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
                move_loading_assistant_to_bottom(&stream_ctx);
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
                move_loading_assistant_to_bottom(&stream_ctx);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(plan_text.chars().count()),
                );
                return;
            }
            if info.kind == "hierarchical_subgoal" {
                let text =
                    build_intent_analysis_main_bubble_text(&info.title, info.detail.as_deref());
                if text.is_empty() {
                    return;
                }
                *current_subgoal_marker.borrow_mut() =
                    extract_subgoal_marker_from_title(&info.title);
                upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
                move_loading_assistant_to_bottom(&stream_ctx);
                answer_delta_chars.set(
                    answer_delta_chars
                        .get()
                        .saturating_add(text.chars().count()),
                );
                return;
            }
            if info.kind == "hierarchical_subgoal_started" {
                let text =
                    build_intent_analysis_main_bubble_text(&info.title, info.detail.as_deref());
                if text.is_empty() {
                    return;
                }
                *current_subgoal_marker.borrow_mut() =
                    extract_subgoal_marker_from_title(&info.title);
                upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
                move_loading_assistant_to_bottom(&stream_ctx);
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
            move_loading_assistant_to_bottom(&stream_ctx);
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
                        created_at: now,
                    });
                }
            });
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
        build_final_response_text, build_hierarchical_plan_main_bubble_text,
        build_intent_analysis_main_bubble_text, enqueue_pending_tool_message_id,
        take_pending_tool_message_id,
    };
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
        let t =
            build_intent_analysis_main_bubble_text("意图分析：Execute", Some("confidence=0.61"));
        assert_eq!(t, "意图分析：Execute\nconfidence=0.61\n\n");
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
}
