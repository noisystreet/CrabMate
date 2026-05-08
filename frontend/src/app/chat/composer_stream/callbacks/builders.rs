//! SSE 回调闭包工厂：`on_tool_result`、`on_timeline_log`、`on_delta`、`on_done`、`on_error`、`on_workspace_changed`、`on_tool_call`。

use std::rc::Rc;

use leptos::prelude::*;

use crate::api::OnToolCallFn;
use crate::i18n;
use crate::message_format::{
    staged_timeline_system_message_body, tool_card_compact_text, tool_card_text,
};
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{TimelineLogInfo, ToolResultInfo};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::timeline_scan::timeline_state_tool;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::super::shell_abort::{clear_abort_slot, user_cancelled_flag};
use super::done_bubble::{DoneBubbleAction, DoneBubbleDecisionInputs, decide_done_bubble_action};
use super::helpers::*;
use super::stream_session_access::{append_stream_assistant_chunk, with_active_session_mut};
use super::stream_turn_state::{
    StreamOutputLaneCell, lane_clear_followup_pending, lane_take_followup_rotation_pending,
};

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
        let tl_ok = info.ok.unwrap_or(true);
        let state = timeline_state_tool(&id, tl_ok);
        let pending_queue = stream_ctx.tail.pending_tool_message_ids();
        let mut updated_existing = false;
        with_active_session_mut(stream_ctx.as_ref(), |s| {
            let tid = info
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty());
            let idx_by_tid = tid.and_then(|t| index_of_loading_tool_by_call_id(&s.messages, t));
            let fifo_id = idx_by_tid
                .is_none()
                .then(|| take_pending_tool_message_id(&pending_queue))
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
            let _ = (preview, full);
            // 与后端 `tool_running` 帧互补：tool_call 往往先于或并列到达，此处立即置位可避免
            // 长耗时工具（如 git_commit）期间状态栏仍误显「模型生成中」。
            stream_ctx.shell.stream.tool_busy.set(true);
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
            let marker = goal_id
                .as_deref()
                .map(|g| format!("hierarchical-subgoal:{g}"))
                .or_else(|| accum.current_subgoal_marker_cloned());
            let tcid = tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            with_active_session_mut(stream_ctx.as_ref(), |s| {
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
                enqueue_pending_tool_message_id(&stream_ctx.tail.pending_tool_message_ids(), id);
            }
        },
    )
}

pub(super) fn make_on_timeline_log(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(TimelineLogInfo)> {
    Rc::new(move |info: TimelineLogInfo| {
        web_sys::console::log_1(&format!("[TL] kind={} title={}", info.kind, info.title).into());
        if info.kind == "final_response" {
            accum.set_saw_final_response_timeline(true);
            stream_ctx.shell.stream.status_busy.set(false);
            let final_text = build_final_response_text(&info.title, info.detail.as_deref());
            if !final_text.is_empty() {
                remove_loading_assistant_placeholder(&stream_ctx);
                if !has_same_assistant_timeline_bubble(&stream_ctx, &final_text) {
                    push_assistant_timeline_bubble(&stream_ctx, final_text.clone(), None);
                    accum.add_answer_delta_chars(final_text.chars().count());
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
            accum.add_answer_delta_chars(intent_text.chars().count());
            return;
        }
        if info.kind == "hierarchical_plan" {
            let plan_text =
                build_hierarchical_plan_main_bubble_text(&info.title, info.detail.as_deref());
            if plan_text.is_empty() {
                return;
            }
            push_assistant_timeline_bubble(&stream_ctx, plan_text.clone(), None);
            accum.add_answer_delta_chars(plan_text.chars().count());
            return;
        }
        if info.kind == "hierarchical_subgoal" || info.kind == "hierarchical_subgoal_started" {
            let text =
                build_hierarchical_subgoal_main_bubble_text(&info.title, info.detail.as_deref());
            if text.is_empty() {
                return;
            }
            accum.set_current_subgoal_marker(extract_subgoal_marker_from_title(&info.title));
            upsert_hierarchical_subgoal_bubble(&stream_ctx, text.clone(), &info.title);
            accum.add_answer_delta_chars(text.chars().count());
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
    output_lane: StreamOutputLaneCell,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |chunk: String| {
        if lane_take_followup_rotation_pending(output_lane.as_ref()) {
            rotate_streaming_assistant_for_followup_model_round(stream_ctx.as_ref());
            accum.clear_answer_delta_chars();
        }
        let mid = stream_ctx.tail.borrow_assistant_id();
        let lane = output_lane.get();
        if lane.in_answer_body_lane() {
            accum.add_answer_delta_chars(chunk.chars().count());
            append_stream_assistant_chunk(stream_ctx.as_ref(), mid.as_str(), &chunk, false);
        } else {
            append_stream_assistant_chunk(stream_ctx.as_ref(), mid.as_str(), &chunk, true);
        }
    })
}

pub(super) fn chat_stream_on_done_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    output_lane: StreamOutputLaneCell,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if user_cancelled_flag(&stream_ctx.shell) {
            lane_clear_followup_pending(output_lane.as_ref());
            clear_abort_slot(&stream_ctx.shell);
            return;
        }
        // 第二次 `assistant_answer_phase` 后若再无正文增量，须在此补做轮换并清零计数器；
        // 否则 `answer_delta_chars` 仍为上一轮时间轴累计，易误判「有输出却无正文」。
        if lane_take_followup_rotation_pending(output_lane.as_ref()) {
            rotate_streaming_assistant_for_followup_model_round(stream_ctx.as_ref());
            accum.clear_answer_delta_chars();
        }
        let turn = accum.summarize_for_stream_done();
        let loc = stream_ctx.locale.get_untracked();
        let mid = stream_ctx.tail.clone_assistant_id();
        with_active_session_mut(stream_ctx.as_ref(), |s| {
            let has_hierarchical_or_tool = s.messages.iter().any(|x| {
                x.is_tool
                    || x.state
                        .as_ref()
                        .is_some_and(|st| st.looks_like_hierarchical_subgoal())
            });
            if let Some(idx) = s.messages.iter().position(|m| m.id == mid)
                && s.messages[idx]
                    .state
                    .as_ref()
                    .is_some_and(|st| st.is_loading())
            {
                s.messages[idx].state = None;
                let body_chars = s.messages[idx].text.chars().count()
                    + s.messages[idx].reasoning_text.chars().count();
                let diag_chars = body_chars.max(turn.answer_delta_chars);
                let body_and_reasoning_empty = s.messages[idx].text.trim().is_empty()
                    && s.messages[idx].reasoning_text.trim().is_empty();
                let end_reason = turn.stream_end_reason.as_deref();
                let in_lane = output_lane.get().in_answer_body_lane();
                match decide_done_bubble_action(DoneBubbleDecisionInputs {
                    body_and_reasoning_empty,
                    end_reason_raw: end_reason,
                    in_answer_body_lane: in_lane,
                    diag_chars,
                    has_hierarchical_or_tool,
                    saw_final_response_timeline: turn.saw_final_response_timeline,
                }) {
                    DoneBubbleAction::Keep => {}
                    DoneBubbleAction::RemoveBubble => {
                        // 含：completed 且有可见增量尾占位、工具型回合空主泡、fallback/final_response 补偿尾戳等。
                        s.messages.remove(idx);
                        return;
                    }
                    DoneBubbleAction::FillMissingFinalHint => {
                        s.messages[idx].text = format!(
                            "{}\n\n{}",
                            i18n::stream_completed_missing_final_summary_hint(loc),
                            i18n::stream_empty_reply_diag_line(
                                loc, end_reason, in_lane, diag_chars
                            )
                        );
                    }
                    DoneBubbleAction::FillDiagnostic => {
                        s.messages[idx].text =
                            build_empty_reply_with_diagnostic(loc, in_lane, diag_chars, end_reason);
                    }
                }
            }
        });
        stream_ctx.shell.stream.status_busy.set(false);
        stream_ctx.shell.stream.tool_busy.set(false);
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
        stream_ctx.chat.clear_stream_resume_handles();
        let mid = stream_ctx.tail.clone_assistant_id();
        let loc = stream_ctx.locale.get_untracked();
        let friendly = build_stream_error_with_suggestion(&msg, loc);
        with_active_session_mut(stream_ctx.as_ref(), |s| {
            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                m.text = friendly.clone();
                m.state = Some(StoredMessageState::Error);
            }
        });
        stream_ctx.shell.stream.status_busy.set(false);
        stream_ctx.shell.stream.tool_busy.set(false);
        stream_ctx.shell.stream.status_err.set(Some(
            i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
        ));
        clear_abort_slot(&stream_ctx.shell);
    })
}

pub(super) fn chat_stream_on_ws_builder(stream_ctx: Rc<ChatStreamCallbackCtx>) -> Rc<dyn Fn()> {
    Rc::new(move || {
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
