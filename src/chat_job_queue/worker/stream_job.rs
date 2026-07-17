//! Web `/chat/stream` 队列任务执行体（从 `worker/mod.rs` 拆出以降低单文件行数）。

use log::{debug, info};

use super::super::stream_finish::{
    StreamJobOutcomeCtx, emit_stream_cancelled_terminal, emit_stream_ended_once,
    stream_job_outcome_after_agent_turn,
};
use super::super::{WebApprovalSession, WebChatJobEnvelope};
use super::JobOutcome;
use super::stream_job_setup::{StreamJobSetupParams, stream_job_setup_runtime};

/// `run_stream_queued_job` 入参（[`WebChatJobEnvelope`] + SSE / 审批）。
pub(super) struct StreamQueuedJobParams {
    pub(super) envelope: WebChatJobEnvelope,
    pub(super) stream_event_tx: tokio::sync::mpsc::Sender<(u64, String)>,
    pub(super) web_approval_session: Option<WebApprovalSession>,
}

pub(super) async fn run_stream_queued_job(p: StreamQueuedJobParams) -> JobOutcome {
    let job_id = p.envelope.job_id;
    info!(
        target: "crabmate",
        "chat stream 任务开始执行 job_id={}",
        job_id
    );
    debug!(
        target: "crabmate",
        "chat stream 执行上下文 job_id={} message_count={} last_user_preview={}",
        job_id,
        p.envelope.messages.len(),
        crate::redact::last_user_message_preview_for_log(&p.envelope.messages)
    );

    let queue_deps = p.envelope.queue_deps.clone();
    let (rt, cancel_watcher) = stream_job_setup_runtime(StreamJobSetupParams {
        envelope: &p.envelope,
        stream_event_tx: p.stream_event_tx,
        web_approval_session: p.web_approval_session,
        queue_deps: queue_deps.as_ref(),
    })
    .await;

    let WebChatJobEnvelope {
        app,
        conversation_id,
        mut messages,
        expected_revision,
        request_agent_role,
        persisted_active_agent_role,
        work_dir,
        workspace_is_set,
        temperature_override,
        seed_override,
        request_audit,
        ..
    } = p.envelope;

    let cfg_snap = {
        let g = queue_deps.cfg.read().await;
        std::sync::Arc::new(g.clone())
    };

    let r = crate::run_agent_turn(crate::RunAgentTurnParams::web_chat_stream(
        crate::WebChatStreamBuildArgs {
            shared: crate::RunAgentTurnSharedInputs {
                client: &queue_deps.client,
                api_key: rt.api_key_turn.as_str(),
                cfg: &rt.cfg_turn,
                tools: rt.tools_for_job.as_slice(),
            },
            messages: &mut messages,
            effective_working_dir: &work_dir,
            workspace_is_set,
            cancel: rt.cancel.clone(),
            per_flight: rt.flight,
            web_tool_ctx: rt.web_tool_ctx.as_ref(),
            temperature_override,
            model_override: None,
            use_executor_model: false,
            executor_model_override: rt.executor_model_override,
            executor_api_base: rt.executor_api_base,
            executor_api_key: rt.executor_api_key,
            seed_override,
            long_term_memory: queue_deps.long_term_memory.clone(),
            job_id,
            conversation_id: conversation_id.as_str(),
            out: &rt.sse_tx,
            turn_allowed_tool_names: rt.turn_allow,
            request_audit: std::sync::Arc::new(request_audit),
            process_handles: std::sync::Arc::clone(&app.aux.process_handles),
        },
    ))
    .await;

    cancel_watcher.abort();
    if let Some(session_id) = rt.approval_session_id.as_deref() {
        app.aux.approval_sessions.write().await.remove(session_id);
    }

    let cancelled_by_signal = rt.cancel.load(std::sync::atomic::Ordering::SeqCst);
    let mut stream_ended_sent = false;
    let (ok, cancelled, err, stream_end_reason) =
        stream_job_outcome_after_agent_turn(StreamJobOutcomeCtx {
            r,
            cancelled_by_signal,
            queue_deps: queue_deps.as_ref(),
            sse_tx: &rt.sse_tx,
            job_id,
            messages: &mut messages,
            cfg_snap: &cfg_snap,
            app: app.as_ref(),
            conversation_id: conversation_id.as_str(),
            expected_revision,
            request_agent_role: request_agent_role.as_deref(),
            persisted_active_agent_role: persisted_active_agent_role.as_deref(),
            stream_ended_sent: &mut stream_ended_sent,
        })
        .await;

    if cancelled {
        emit_stream_cancelled_terminal(&rt.sse_tx, job_id).await;
    }
    if !stream_ended_sent {
        let tiktoken_prompt_tokens =
            crate::agent::tiktoken_prompt_tokens::prompt_token_count_vendor_shaped_for_session(
                &cfg_snap, &messages,
            );
        emit_stream_ended_once(
            &rt.sse_tx,
            job_id,
            stream_end_reason,
            &mut stream_ended_sent,
            "chat_job_queue::stream stream_ended",
            tiktoken_prompt_tokens,
        )
        .await;
    }
    drop(rt.sse_tx);
    queue_deps.sse_stream_hub.remove_job(job_id);
    JobOutcome::Stream { ok, cancelled, err }
}
