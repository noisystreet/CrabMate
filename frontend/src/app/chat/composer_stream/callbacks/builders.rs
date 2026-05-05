//! SSE 回调闭包工厂：`on_tool_result`、`on_timeline_log`、`on_delta`、`on_done`、`on_error`、`on_workspace_changed`、`on_tool_call`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crabmate_sse_protocol::StreamEndReason;
use leptos::prelude::*;

use crate::api::OnToolCallFn;
use crate::i18n;
use crate::message_format::{
    staged_timeline_system_message_body, tool_card_compact_text, tool_card_text,
};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{TimelineLogInfo, ToolResultInfo};
use crate::storage::StoredMessage;
use crate::timeline_scan::timeline_state_tool;

use super::super::context::ChatStreamCallbackCtx;
use super::super::shell_abort::{clear_abort_slot, user_cancelled_flag};
use super::helpers::*;

pub(super) fn make_on_tool_result(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(ToolResultInfo)> {
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
                        m.tool_call_id = info.tool_call_id.clone().filter(|x| !x.trim().is_empty());
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
                        tool_call_id: info.tool_call_id.clone().filter(|x| !x.trim().is_empty()),
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
}

pub(super) fn chat_stream_on_tool_call_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    current_subgoal_marker: Rc<RefCell<Option<String>>>,
) -> OnToolCallFn {
    Rc::new(
        move |name: String,
              summary: String,
              preview: Option<String>,
              full: Option<String>,
              goal_id: Option<String>,
              tool_call_id: Option<String>| {
            let _ = (preview, full);
            // 与后端 `tool_running` 帧互补：tool_call 往往先于或并列到达，此处立即置位可避免
            // 长耗时工具（如 git_commit）期间状态栏仍误显「模型生成中」。
            stream_ctx.shell.tool_busy.set(true);
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
}

pub(super) fn make_on_timeline_log(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    answer_delta_chars: Rc<Cell<usize>>,
    current_subgoal_marker: Rc<RefCell<Option<String>>>,
    saw_final_response_timeline: Rc<Cell<bool>>,
) -> Rc<dyn Fn(TimelineLogInfo)> {
    Rc::new(move |info: TimelineLogInfo| {
        web_sys::console::log_1(&format!("[TL] kind={} title={}", info.kind, info.title).into());
        if info.kind == "final_response" {
            saw_final_response_timeline.set(true);
            stream_ctx.shell.status_busy.set(false);
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
            } else {
                // 补偿收尾可能带空 final_response；若不撤 loading，on_done 会误报「未收到正文片段」。
                remove_loading_assistant_placeholder(&stream_ctx);
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
        if info.kind == "hierarchical_subgoal" || info.kind == "hierarchical_subgoal_started" {
            let text =
                build_hierarchical_subgoal_main_bubble_text(&info.title, info.detail.as_deref());
            if text.is_empty() {
                return;
            }
            *current_subgoal_marker.borrow_mut() = extract_subgoal_marker_from_title(&info.title);
            upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
            answer_delta_chars.set(
                answer_delta_chars
                    .get()
                    .saturating_add(text.chars().count()),
            );
            return;
        }
        if info.kind == "tool_step_started" || info.kind == "tool_step_finished" {
            return;
        }
        let mut body = info.title.trim().to_string();
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
}

pub(super) fn chat_stream_on_delta_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
    answer_delta_chars: Rc<Cell<usize>>,
    pending_followup_answer_round: Rc<Cell<bool>>,
) -> Rc<dyn Fn(String)> {
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
}

pub(super) fn chat_stream_on_done_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    in_answer_phase: Rc<Cell<bool>>,
    answer_delta_chars: Rc<Cell<usize>>,
    pending_followup_answer_round: Rc<Cell<bool>>,
    stream_end_reason: Rc<RefCell<Option<String>>>,
    saw_final_response_timeline: Rc<Cell<bool>>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if user_cancelled_flag(&stream_ctx.shell) {
            pending_followup_answer_round.set(false);
            clear_abort_slot(&stream_ctx.shell);
            return;
        }
        // 第二次 `assistant_answer_phase` 后若再无正文增量，须在此补做轮换并清零计数器；
        // 否则 `answer_delta_chars` 仍为上一轮时间轴累计，易误判「有输出却无正文」。
        let had_pending_followup = pending_followup_answer_round.get();
        pending_followup_answer_round.set(false);
        if had_pending_followup {
            rotate_streaming_assistant_for_followup_model_round(stream_ctx.as_ref());
            answer_delta_chars.set(0);
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
                            .and_then(|s| s.parse::<StreamEndReason>().ok())
                            .is_some_and(|r| r == StreamEndReason::Completed)
                            && in_answer_phase.get()
                            && diag_chars > 0;
                        if completed_with_visible_delta {
                            // 流程已完成且本轮存在可见输出时，空 loading 气泡多为尾部占位残留，直接删除避免误报“无回复”。
                            s.messages.remove(idx);
                            return;
                        }
                        let drop_empty_main_after_tool_like_turn = end_reason
                            .as_deref()
                            .and_then(|s| s.parse::<StreamEndReason>().ok())
                            .is_some_and(|r| {
                                matches!(
                                    r,
                                    StreamEndReason::Completed
                                        | StreamEndReason::Fallback
                                        | StreamEndReason::Cancelled
                                        | StreamEndReason::NoOutput
                                )
                            })
                            && has_hierarchical_or_tool
                            && (!in_answer_phase.get() || diag_chars == 0);
                        if drop_empty_main_after_tool_like_turn {
                            // 工具/子目标已占满可见输出；主占位虽可能已进终答相却无任何正文增量（diag=0），删空尾泡。
                            s.messages.remove(idx);
                            return;
                        }
                        let drop_redundant_empty_after_fallback_timeline = (end_reason
                            .as_deref()
                            .and_then(|r| r.parse::<StreamEndReason>().ok())
                            == Some(StreamEndReason::Fallback)
                            || saw_final_response_timeline.get())
                            && in_answer_phase.get()
                            && diag_chars == 0;
                        if drop_redundant_empty_after_fallback_timeline {
                            // 补偿收尾 / final_response 时间轴：正文应在时间轴气泡或已随 placeholder 移除；空尾戳无须「无正文片段」。
                            s.messages.remove(idx);
                            return;
                        }
                        let completed_no_final = should_show_missing_final_summary_hint(
                            end_reason.as_deref(),
                            in_answer_phase.get(),
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
        stream_ctx.shell.tool_busy.set(false);
        clear_abort_slot(&stream_ctx.shell);
    })
}

pub(super) fn chat_stream_on_error_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |msg: String| {
        if user_cancelled_flag(&stream_ctx.shell) {
            clear_abort_slot(&stream_ctx.shell);
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
        stream_ctx.shell.tool_busy.set(false);
        stream_ctx.shell.status_err.set(Some(
            i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
        ));
        clear_abort_slot(&stream_ctx.shell);
    })
}

pub(super) fn chat_stream_on_ws_builder(stream_ctx: Rc<ChatStreamCallbackCtx>) -> Rc<dyn Fn()> {
    Rc::new(move || {
        (stream_ctx.shell.refresh_workspace)();
        if stream_ctx.shell.changelist_modal_open.get_untracked() {
            stream_ctx
                .shell
                .changelist_fetch_nonce
                .update(|x| *x = x.wrapping_add(1));
        }
    })
}
