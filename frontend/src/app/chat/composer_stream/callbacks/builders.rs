//! SSE 回调闭包工厂：`on_tool_result`、`on_timeline_log`、`on_done`、`on_error`、`on_workspace_changed`、`on_tool_call`（`on_delta` 见 [`super::delta_apply`]）。

use std::rc::Rc;

use leptos::prelude::*;

use crate::api::OnToolCallFn;
use crate::app::stream_shell_busy::StreamShellBusyOp;
use crate::i18n;
use crate::message_format::{
    staged_timeline_system_message_body, strip_ansi_codes, tool_card_compact_text, tool_card_text,
};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{TimelineLogInfo, ToolOutputChunkInfo, ToolResultInfo};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::stream_overlay_take_into_stored_message;
use crate::timeline_scan::timeline_state_tool;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::shell_abort::{clear_abort_slot, user_cancelled_flag};
use super::done_session::apply_stream_done_to_loading_assistant;
use super::error_session::apply_stream_error_on_messages;
use super::helpers::*;

pub(super) fn make_on_tool_output_chunk(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(ToolOutputChunkInfo)> {
    Rc::new(move |info: ToolOutputChunkInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        let tid = info.tool_call_id.trim();
        if tid.is_empty() {
            return;
        }
        stream_ctx.update_bound_session(|s| {
            let idx_opt = index_of_tool_message_by_call_id_latest(&s.messages, tid);
            if let Some(idx) = idx_opt {
                if info.name.as_deref() == Some("terminal_session") {
                    s.messages[idx]
                        .reasoning_text
                        .push_str(&strip_ansi_codes(&info.chunk));
                } else {
                    s.messages[idx].reasoning_text.push_str(&info.chunk);
                }
            }
        });
    })
}

pub(super) fn make_on_tool_result(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(ToolResultInfo)> {
    Rc::new(move |info: ToolResultInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        let loc = stream_ctx.locale.get_untracked();
        let result_text = tool_card_text(&info, loc);
        let compact = tool_card_compact_text(&info, loc);
        let t = to_single_line(&compact, 180);
        let detail = result_text.clone();

        let id = make_message_id();
        let tl_ok = info.ok.unwrap_or(true);
        let state = timeline_state_tool(&id, tl_ok);
        let stream_ctx_rc = Rc::clone(&stream_ctx);
        let mut updated_existing = false;
        stream_ctx.update_bound_session(|s| {
            let tid = info
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty());
            let idx_by_tid = tid.and_then(|t| index_of_loading_tool_by_call_id(&s.messages, t));
            let fifo_id = idx_by_tid
                .is_none()
                .then(|| stream_ctx_rc.scratch.take_pending_tool_fifo_head())
                .flatten();
            let idx_opt = idx_by_tid.or_else(|| {
                fifo_id
                    .as_deref()
                    .and_then(|pid| index_of_message_id(&s.messages, pid))
            });
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
                    if let Some(idx) = s.messages.iter().rposition(|m| {
                        m.state
                            .as_ref()
                            .is_some_and(|st| st.matches_full_marker(marker.as_str()))
                    }) {
                        s.messages.insert(idx + 1, msg);
                    } else {
                        s.messages.push(msg);
                    }
                } else {
                    s.messages.push(msg);
                }
            }
        });
        ensure_streaming_assistant_tail_last(&stream_ctx);
    })
}

pub(super) fn chat_stream_on_tool_call_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> OnToolCallFn {
    Rc::new(
        move |name: String,
              summary: String,
              preview: Option<String>,
              full: Option<String>,
              goal_id: Option<String>,
              tool_call_id: Option<String>| {
            if stream_ctx.is_stale() {
                return;
            }
            let _ = (preview, full);
            // 与后端 `tool_running` 帧互补：tool_call 往往先于或并列到达，此处立即置位可避免
            // 长耗时工具（如 git_commit）期间状态栏仍误显「模型生成中」。
            stream_ctx
                .shell
                .stream
                .apply_busy_op(StreamShellBusyOp::MirrorToolRunning(true));
            let loc = stream_ctx.locale.get_untracked();
            let core = if !summary.trim().is_empty() {
                summary.trim().to_string()
            } else if !name.trim().is_empty() {
                format!("{}{}", i18n::tool_card_prefix(loc), name.trim())
            } else {
                i18n::tool_card_fallback(loc).to_string()
            };
            // 工具占位气泡已有 `msg-loading` 呼吸边框；不再拼接「· 工具执行中…」，避免终态摘要重复该文案。
            let text = to_single_line(&core, 140);
            let detail = if !name.trim().is_empty() {
                format!("tool: {name}\nstatus: running")
            } else {
                "status: running".to_string()
            };
            let id = make_message_id();
            let marker = goal_id
                .as_deref()
                .map(|g| format!("hierarchical-subgoal:{g}"))
                .or_else(|| accum.current_subgoal_marker_cloned());
            let tcid = tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            stream_ctx.update_bound_session(|s| {
                let msg = StoredMessage {
                    id: id.clone(),
                    role: "system".to_string(),
                    text,
                    reasoning_text: detail.clone(),
                    image_urls: vec![],
                    state: Some(StoredMessageState::Loading),
                    is_tool: true,
                    tool_call_id: tcid.clone(),
                    tool_name: non_empty_trimmed_tool_name(&name),
                    created_at: message_created_ms(),
                };
                if let Some(mk) = marker.as_deref()
                    && let Some(idx) = s.messages.iter().rposition(|m| {
                        m.state
                            .as_ref()
                            .is_some_and(|st| st.matches_full_marker(mk))
                    })
                {
                    s.messages.insert(idx + 1, msg);
                } else {
                    s.messages.push(msg);
                }
            });
            // 开场白留在时间线/工具之上；工具后挂新占位，续写走新气泡，避免“最早的话出现在最下面”。
            finalize_loading_assistant_before_tool_and_tail_with_new_loading(&stream_ctx, &id);
            // 有 `tool_call_id` 时由 `tool_result` 按 id 命中占位气泡；否则保持 FIFO。
            if tcid.is_none() {
                stream_ctx.scratch.enqueue_pending_tool_message_id(id);
            }
        },
    )
}

fn timeline_log_dispatch_final_response(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    accum.set_saw_final_response_timeline(true);
    stream_ctx
        .shell
        .stream
        .apply_busy_op(StreamShellBusyOp::ReleaseStreamingStatusAfterTimelineFinal);
    let final_text = build_final_response_text(&info.title, info.detail.as_deref());
    if !final_text.is_empty() {
        remove_loading_assistant_placeholder(stream_ctx);
        if !has_same_assistant_timeline_bubble(stream_ctx, &final_text) {
            push_assistant_timeline_bubble(stream_ctx, final_text.clone(), None);
            accum.add_answer_delta_chars(final_text.chars().count());
        }
    } else {
        // 补偿收尾可能带空 final_response；若不撤 loading，on_done 会误报「未收到正文片段」。
        remove_loading_assistant_placeholder(stream_ctx);
    }
}

fn timeline_log_dispatch_intent_analysis(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let intent_text = build_intent_analysis_main_bubble_text(&info.title, info.detail.as_deref());
    if intent_text.is_empty() {
        return;
    }
    push_assistant_timeline_bubble(stream_ctx, intent_text.clone(), None);
    accum.add_answer_delta_chars(intent_text.chars().count());
}

fn timeline_log_dispatch_hierarchical_plan(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let plan_text = build_hierarchical_plan_main_bubble_text(&info.title, info.detail.as_deref());
    if plan_text.is_empty() {
        return;
    }
    push_assistant_timeline_bubble(stream_ctx, plan_text.clone(), None);
    accum.add_answer_delta_chars(plan_text.chars().count());
}

fn timeline_log_dispatch_hierarchical_subgoal(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: &TimelineLogInfo,
) {
    let text = build_hierarchical_subgoal_main_bubble_text(&info.title, info.detail.as_deref());
    if text.is_empty() {
        return;
    }
    accum.set_current_subgoal_marker(extract_subgoal_marker_from_title(&info.title));
    upsert_hierarchical_subgoal_bubble(stream_ctx, text.clone(), &info.title);
    accum.add_answer_delta_chars(text.chars().count());
}

fn timeline_log_dispatch_default_body(stream_ctx: &ChatStreamCallbackCtx, info: &TimelineLogInfo) {
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
    push_assistant_timeline_bubble(stream_ctx, staged_timeline_system_message_body(&body), None);
}

fn timeline_log_dispatch_body(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
    info: TimelineLogInfo,
) {
    web_sys::console::log_1(&format!("[TL] kind={} title={}", info.kind, info.title).into());
    match info.kind.as_str() {
        "final_response" => timeline_log_dispatch_final_response(stream_ctx, accum, &info),
        "intent_analysis" => timeline_log_dispatch_intent_analysis(stream_ctx, accum, &info),
        "hierarchical_plan" => timeline_log_dispatch_hierarchical_plan(stream_ctx, accum, &info),
        "hierarchical_subgoal" | "hierarchical_subgoal_started" => {
            timeline_log_dispatch_hierarchical_subgoal(stream_ctx, accum, &info);
        }
        "tool_step_started" | "tool_step_finished" => {}
        _ => timeline_log_dispatch_default_body(stream_ctx, &info),
    }
}

pub(super) fn make_on_timeline_log(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(TimelineLogInfo)> {
    Rc::new(move |info: TimelineLogInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        timeline_log_dispatch_body(&stream_ctx, accum.as_ref(), info);
    })
}

pub(super) fn chat_stream_on_done_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if user_cancelled_flag(&stream_ctx.shell) {
            stream_ctx.scratch.clear_followup_pending();
            clear_abort_slot(&stream_ctx.shell);
            return;
        }
        if stream_ctx.is_stale() {
            return;
        }
        // 第二次 `assistant_answer_phase` 后若再无正文增量，须在此补做轮换并清零计数器；
        // 否则 `answer_delta_chars` 仍为上一轮时间轴累计，易误判「有输出却无正文」。
        if stream_ctx.scratch.take_followup_rotation_pending() {
            rotate_streaming_assistant_for_followup_model_round(stream_ctx.as_ref());
            accum.clear_answer_delta_chars();
        }
        let turn = accum.summarize_for_stream_done();
        let loc = stream_ctx.locale.get_untracked();
        let mid = stream_ctx.scratch.clone_assistant_id();
        stream_ctx.update_bound_session(|s| {
            let sid = stream_ctx.bound_stream_session_id.as_str();
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid,
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            apply_stream_done_to_loading_assistant(
                &mut s.messages,
                mid.as_str(),
                &turn,
                stream_ctx
                    .scratch
                    .current_output_lane()
                    .in_answer_body_lane(),
                loc,
            );
        });
        stream_ctx
            .shell
            .stream
            .apply_busy_op(StreamShellBusyOp::ReleaseTurnShellBusy);
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
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.chat.clear_stream_resume_handles();
        let mid = stream_ctx.scratch.clone_assistant_id();
        let loc = stream_ctx.locale.get_untracked();
        let friendly = build_stream_error_with_suggestion(&msg, loc);
        stream_ctx.update_bound_session(|s| {
            let sid = stream_ctx.bound_stream_session_id.as_str();
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
                stream_overlay_take_into_stored_message(
                    stream_ctx.chat.stream_text_overlay,
                    sid,
                    mid.as_str(),
                    &mut s.messages[idx],
                );
            }
            apply_stream_error_on_messages(&mut s.messages, mid.as_str(), friendly, loc);
        });
        stream_ctx
            .shell
            .stream
            .apply_busy_op(StreamShellBusyOp::ReleaseTurnShellBusy);
        stream_ctx.shell.stream.status_err.set(Some(
            i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
        ));
        clear_abort_slot(&stream_ctx.shell);
    })
}

pub(super) fn chat_stream_on_ws_builder(stream_ctx: Rc<ChatStreamCallbackCtx>) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if stream_ctx.is_stale() {
            return;
        }
        (stream_ctx.shell.refresh_workspace)();
        if stream_ctx.shell.modal.changelist_modal_open.get_untracked() {
            stream_ctx
                .shell
                .modal
                .changelist_fetch_nonce
                .update(|x| *x = x.wrapping_add(1));
        }
    })
}
