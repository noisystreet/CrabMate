//! 装配 [`crate::api::ChatStreamCallbacks`]：集中各 `on_*` 闭包。

use std::rc::Rc;

use leptos::prelude::*;

use crate::api::ChatStreamCallbacks;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::message_format::staged_timeline_system_message_body;
use crate::session_ops::{make_message_id, message_created_ms};
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, ThinkingTraceInfo,
};
use crate::storage::StoredMessage;
use crate::timeline_scan::{timeline_state_staged_end, timeline_state_staged_start};

use super::super::context::ChatStreamCallbackCtx;
use super::super::shell_abort::clear_abort_slot;
use super::super::stream_sse_scratch::StreamSseScratch;
use super::builders::*;
use super::delta_apply::chat_stream_on_delta_builder;
use super::helpers::*;
use super::stream_session_access::with_active_session_mut;
use super::stream_turn_state::{StreamOutputLaneCell, lane_on_assistant_answer_phase};

/// 由 [`super::super::make_attach_chat_stream`](super::super::make_attach_chat_stream) 调用；集中所有 `on_*` 闭包，降低父模块维护面。
pub(crate) fn build_chat_stream_callbacks(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    scratch: StreamSseScratch,
) -> ChatStreamCallbacks {
    let lane: StreamOutputLaneCell = scratch.lane.clone();
    let accum = Rc::clone(&scratch.accum);
    let on_delta: Rc<dyn Fn(String)> =
        chat_stream_on_delta_builder(Rc::clone(&stream_ctx), Rc::clone(&lane), Rc::clone(&accum));

    let on_done: Rc<dyn Fn()> =
        chat_stream_on_done_builder(Rc::clone(&stream_ctx), Rc::clone(&lane), Rc::clone(&accum));

    let on_error: Rc<dyn Fn(String)> = chat_stream_on_error_builder(Rc::clone(&stream_ctx));

    let on_ws: Rc<dyn Fn()> = chat_stream_on_ws_builder(Rc::clone(&stream_ctx));

    let on_tool_call = chat_stream_on_tool_call_builder(Rc::clone(&stream_ctx), Rc::clone(&accum));

    let on_tool_status: Rc<dyn Fn(bool)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |b: bool| {
            stream_ctx.shell.stream.tool_busy.set(b);
        })
    };

    let on_tool_result = make_on_tool_result(Rc::clone(&stream_ctx));

    let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |req: CommandApprovalRequest| {
            stream_ctx.shell.approval.pending_approval.set(Some((
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
            with_active_session_mut(stream_ctx.as_ref(), |s| {
                s.server_conversation_id = Some(id);
                s.server_revision = None;
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
            with_active_session_mut(stream_ctx.as_ref(), |s| {
                s.server_revision = Some(rev);
            });
        })
    };

    let on_stream_ended: Rc<dyn Fn(String)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        let accum = Rc::clone(&accum);
        Rc::new(move |reason: String| {
            accum.set_stream_end_reason(reason.clone());
            stream_ctx.chat.clear_stream_resume_handles();
            // `stream_ended` 表示服务端已结束本轮流式任务：无论 `reason` 是否能解析为已知枚举，
            // 都应回落 busy，避免状态栏长期停在「模型生成中」。（未知 reason 仍写入 stream_end_reason 供 diagnostics。）
            stream_ctx.shell.stream.status_busy.set(false);
            clear_abort_slot(&stream_ctx.shell);
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
        let lane = Rc::clone(&lane);
        Rc::new(move || {
            // 重复 answer_phase 将车道切入 PendingFollowup；轮换由 `on_delta` / `on_done` 消费。
            lane_on_assistant_answer_phase(lane.as_ref());
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
            let now = message_created_ms();
            let state = timeline_state_staged_start(&id, info.step_index, info.total_steps);
            with_active_session_mut(stream_ctx.as_ref(), |s| {
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
            });
            ensure_streaming_assistant_tail_last(&stream_ctx);
        })
    };

    let on_clarification: Rc<dyn Fn(ClarificationQuestionnaireInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ClarificationQuestionnaireInfo| {
            stream_ctx
                .shell
                .approval
                .pending_clarification
                .set(Some(PendingClarificationForm::from_sse(info)));
        })
    };

    let on_timeline_log = make_on_timeline_log(Rc::clone(&stream_ctx), Rc::clone(&accum));

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
            let now = message_created_ms();
            let state =
                timeline_state_staged_end(&id, info.step_index, info.total_steps, &info.status);
            with_active_session_mut(stream_ctx.as_ref(), |s| {
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
            });
            ensure_streaming_assistant_tail_last(&stream_ctx);
        })
    };

    // thinking_trace 写入侧栏调试台（`thinking_trace_log`），不进聊天正文。
    const MAX_THINKING_TRACE_ENTRIES: usize = 512;
    let on_thinking_trace: Rc<dyn Fn(ThinkingTraceInfo)> = {
        let stream_ctx = Rc::clone(&stream_ctx);
        Rc::new(move |info: ThinkingTraceInfo| {
            #[cfg(debug_assertions)]
            web_sys::console::log_1(&format!("thinking_trace {:?}", info).into());
            stream_ctx.shell.approval.thinking_trace_log.update(|v| {
                v.push(info);
                let overflow = v.len().saturating_sub(MAX_THINKING_TRACE_ENTRIES);
                if overflow > 0 {
                    v.drain(..overflow);
                }
            });
        })
    };

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
