//! 默认 Agent 外层循环：P → R → E，直至结束。

use std::sync::atomic::Ordering;

use log::{debug, info};

use crate::agent::per_coord::PerCoordinator;
use crate::sse::{SseErrorBody, SsePayload, encode_message};
use crate::types::USER_CANCELLED_FINISH_REASON;

use super::execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
use super::messages::push_assistant_merging_trailing_empty_placeholder;
use super::params::RunLoopParams;
use super::plan_call::{PerPlanCallModelParams, per_plan_call_model_retrying};
use super::reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};

pub(crate) async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    'outer: loop {
        if sse_sender_closed(p.out) {
            info!(target: "crabmate", "SSE sender closed, aborting run_agent_turn loop early");
            break;
        }
        if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break;
        }

        let render_to_terminal = p.render_to_terminal;
        if let Some(ref ltm) = p.long_term_memory {
            ltm.prepare_messages(
                p.cfg.as_ref(),
                p.long_term_memory_scope_id.as_deref(),
                p.messages,
            );
        }
        crate::agent::context_window::prepare_messages_for_model(
            p.llm_backend,
            p.client,
            p.api_key,
            p.cfg.as_ref(),
            p.messages,
            Some(per_coord),
            p.workspace_changelist.as_ref().map(|a| a.as_ref()),
        )
        .await?;
        let (msg, finish_reason) = per_plan_call_model_retrying(PerPlanCallModelParams {
            llm_backend: p.llm_backend,
            client: p.client,
            api_key: p.api_key,
            cfg: p.cfg.as_ref(),
            tools_defs: p.tools_defs,
            messages: p.messages,
            out: p.out,
            render_to_terminal,
            no_stream: p.no_stream,
            cancel: p.cancel,
            plain_terminal_stream: p.plain_terminal_stream,
            temperature_override: p.temperature_override,
            seed_override: p.seed_override,
            request_chrome_trace: p.request_chrome_trace.clone(),
        })
        .await?;
        if let Some(f) = p.per_flight.as_ref() {
            f.awaiting_plan_rewrite_model
                .store(false, Ordering::Relaxed);
        }
        debug!(
            target: "crabmate",
            "模型轮次输出 finish_reason={} message_count_before_push={} assistant_preview={}",
            finish_reason,
            p.messages.len(),
            crate::redact::assistant_message_preview_for_log(&msg)
        );
        push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            break;
        }

        match per_reflect_after_assistant(per_coord, &finish_reason, &msg, p.messages) {
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                break;
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                continue 'outer;
            }
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.out {
                    let _ = crate::sse::send_string_logged(
                        tx,
                        encode_message(SsePayload::Error(SseErrorBody {
                            error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
                            code: Some("plan_rewrite_exhausted".to_string()),
                        })),
                        "outer_loop::plan_rewrite_exhausted",
                    )
                    .await;
                }
                break;
            }
        }

        let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
        let echo_terminal_transcript = render_to_terminal && p.out.is_none();
        let exec_outcome = per_execute_tools_web(
            tool_calls,
            per_coord,
            p.messages,
            WebExecuteCtx {
                cfg: p.cfg,
                effective_working_dir: p.effective_working_dir,
                workspace_is_set: p.workspace_is_set,
                read_file_turn_cache: p.read_file_turn_cache.clone(),
                out: p.out,
                web_tool_ctx: p.web_tool_ctx,
                cli_tool_ctx: p.cli_tool_ctx,
                echo_terminal_transcript,
                mcp_session: p.mcp_session.as_ref(),
                workspace_changelist: p.workspace_changelist.as_ref(),
                request_chrome_trace: p.request_chrome_trace.clone(),
            },
        )
        .await;
        if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            break;
        }
        if let Some(f) = p.per_flight.as_ref() {
            f.sync_from_per_coord(per_coord);
        }
    }
    Ok(())
}
