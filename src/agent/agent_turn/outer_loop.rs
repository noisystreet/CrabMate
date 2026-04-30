//! 默认 Agent 外层循环：P → R → E，直至结束。

use std::sync::atomic::Ordering;

use log::{debug, info};

use crate::agent::per_coord::PerCoordinator;
use crate::sse::{SsePayload, encode_message};
use crate::types::{Message, USER_CANCELLED_FINISH_REASON, is_intent_gate_ephemeral_system};

use super::errors::{
    AgentTurnSubPhase, RunAgentTurnError, TurnAbortReason, sse_plan_rewrite_exhausted_body,
};
use super::execute_tools::{
    ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web, sse_sender_closed,
};
use super::messages::push_assistant_merging_trailing_empty_placeholder;
use super::params::RunLoopParams;
use super::plan::{PerPlanCallModelParams, per_plan_call_model_retrying};
use super::reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
use super::sub_agent_policy::filter_tool_defs_for_executor_kind;

const MAX_OUTER_LOOP_ITERATIONS_SAFETY: u32 = 40;

pub(crate) async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let start_time = std::time::Instant::now();
    let mut is_first_iteration = true;
    let mut iteration_count: u32 = 0;
    'outer: loop {
        iteration_count = iteration_count.saturating_add(1);
        if iteration_count > MAX_OUTER_LOOP_ITERATIONS_SAFETY {
            return Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: format!(
                    "达到外层循环安全上限（{} 轮），已中止以避免重复工具调用死循环",
                    MAX_OUTER_LOOP_ITERATIONS_SAFETY
                ),
            });
        }
        if p.ctx.cfg.max_turn_duration_seconds > 0
            && start_time.elapsed().as_secs() > p.ctx.cfg.max_turn_duration_seconds
        {
            return Err(RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Planner,
                message: format!(
                    "已达到单轮墙钟时间上限 ({}秒)",
                    p.ctx.cfg.max_turn_duration_seconds
                ),
            });
        }
        // 第一轮使用 planner_model（use_executor_model=false），后续轮次使用 executor_model
        p.turn.use_executor_model = !is_first_iteration;
        if is_first_iteration {
            is_first_iteration = false;
        }
        p.turn.sub_phase = AgentTurnSubPhase::Planner;
        if let Some(ref t) = p.ctx.tracing_chat_turn {
            t.on_outer_loop_iteration();
        }
        if sse_sender_closed(p.ctx.out) {
            info!(target: "crabmate", "SSE sender closed, aborting run_agent_turn loop early");
            return Err(RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Planner,
                reason: TurnAbortReason::SseDisconnected,
            });
        }
        if p.ctx.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Planner,
                reason: TurnAbortReason::UserCancelled,
            });
        }

        let render_to_terminal = p.ctx.render_to_terminal;
        if let Some(hint) = p.turn.intent_turn_gate_hint.take() {
            p.turn.messages.push(Message::system_intent_gate_hint(hint));
        }
        if let Some(ref ltm) = p.ctx.long_term_memory {
            ltm.prepare_messages(
                p.ctx.cfg.as_ref(),
                p.ctx.long_term_memory_scope_id.as_deref(),
                p.turn.messages,
            );
        }
        crate::agent::context_window::prepare_messages_for_model(
            p.ctx.llm_backend,
            p.ctx.client,
            p.ctx.api_key,
            p.ctx.cfg.as_ref(),
            p.turn.messages,
            Some(per_coord),
            p.ctx.workspace_changelist.as_ref().map(|a| a.as_ref()),
        )
        .await
        .map_err(|e| {
            p.turn
                .messages
                .retain(|m| !is_intent_gate_ephemeral_system(m));
            RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: e.to_string(),
            }
        })?;
        let tools_for_call: Vec<crate::types::Tool> = match p.turn.step_executor_constraint {
            Some(k) => {
                let mut v =
                    filter_tool_defs_for_executor_kind(p.ctx.tools_defs, p.ctx.cfg.as_ref(), k);
                if let Some(ref allow) = p.ctx.turn_allowed_tool_names {
                    let mcp_ok = allow.contains("mcp");
                    v.retain(|t| {
                        let n = t.function.name.as_str();
                        if n.starts_with("mcp__") {
                            mcp_ok
                        } else {
                            allow.contains(n)
                        }
                    });
                }
                v
            }
            None => Vec::new(),
        };
        let tools_defs_slice: &[crate::types::Tool] = if p.turn.step_executor_constraint.is_some() {
            tools_for_call.as_slice()
        } else {
            p.ctx.tools_defs
        };
        let (msg, finish_reason) = per_plan_call_model_retrying(PerPlanCallModelParams {
            llm_backend: p.ctx.llm_backend,
            client: p.ctx.client,
            api_key: p.ctx.api_key,
            cfg: p.ctx.cfg.as_ref(),
            tools_defs: tools_defs_slice,
            messages: p.turn.messages,
            out: p.ctx.out,
            render_to_terminal,
            no_stream: p.ctx.no_stream,
            cancel: p.ctx.cancel,
            plain_terminal_stream: p.ctx.plain_terminal_stream,
            temperature_override: p.turn.temperature_override,
            seed_override: p.turn.seed_override,
            request_chrome_trace: p.ctx.request_chrome_trace.clone(),
            model_override: p.effective_model(),
            executor_api_base: if p.turn.use_executor_model {
                p.turn.executor_api_base.as_deref()
            } else {
                None
            },
            executor_api_key: if p.turn.use_executor_model {
                p.turn.executor_api_key.as_deref()
            } else {
                None
            },
        })
        .await
        .map_err(|e| {
            p.turn
                .messages
                .retain(|m| !is_intent_gate_ephemeral_system(m));
            RunAgentTurnError::from_llm(AgentTurnSubPhase::Planner, e)
        })?;
        p.turn
            .messages
            .retain(|m| !is_intent_gate_ephemeral_system(m));
        if let Some(f) = p.ctx.per_flight.as_ref() {
            f.awaiting_plan_rewrite_model
                .store(false, Ordering::Relaxed);
        }
        debug!(
            target: "crabmate",
            "模型轮次输出 finish_reason={} message_count_before_push={} assistant_preview={}",
            finish_reason,
            p.turn.messages.len(),
            crate::redact::assistant_message_preview_for_log(&msg)
        );
        push_assistant_merging_trailing_empty_placeholder(p.turn.messages, msg.clone());
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            break;
        }

        match per_reflect_after_assistant(p, per_coord, &finish_reason, &msg).await {
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.ctx.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                break;
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.ctx.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                continue 'outer;
            }
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                if let Some(f) = p.ctx.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted { reason } => {
                if let Some(f) = p.ctx.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.ctx.out {
                    let _ = crate::sse::send_string_logged(
                        tx,
                        encode_message(SsePayload::Error(sse_plan_rewrite_exhausted_body(
                            p.ctx.tracing_chat_turn.as_ref(),
                            reason.as_str(),
                        ))),
                        "outer_loop::plan_rewrite_exhausted",
                    )
                    .await;
                }
                break;
            }
        }

        let tool_calls = msg
            .tool_calls
            .as_ref()
            .ok_or_else(|| RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Executor,
                message: "无 tool_calls".to_string(),
            })?;
        p.turn.sub_phase = AgentTurnSubPhase::Executor;
        let echo_terminal_transcript = render_to_terminal && p.ctx.out.is_none();
        let exec_outcome = per_execute_tools_web(
            tool_calls,
            per_coord,
            p.turn.messages,
            WebExecuteCtx {
                cfg: p.ctx.cfg,
                effective_working_dir: p.ctx.effective_working_dir,
                workspace_is_set: p.ctx.workspace_is_set,
                read_file_turn_cache: p.ctx.read_file_turn_cache.clone(),
                out: p.ctx.out,
                web_tool_ctx: p.ctx.web_tool_ctx,
                cli_tool_ctx: p.ctx.cli_tool_ctx,
                echo_terminal_transcript,
                mcp_session: p.ctx.mcp_session.as_ref(),
                workspace_changelist: p.ctx.workspace_changelist.as_ref(),
                request_chrome_trace: p.ctx.request_chrome_trace.clone(),
                step_executor_constraint: p.turn.step_executor_constraint,
                tools_defs_full: p.ctx.tools_defs,
                turn_allow: p.ctx.turn_allowed_tool_names.as_ref().map(|a| a.as_ref()),
                long_term_memory: p.ctx.long_term_memory.clone(),
                long_term_memory_scope_id: p.ctx.long_term_memory_scope_id.clone(),
                tracing_chat_turn: p.ctx.tracing_chat_turn.clone(),
            },
        )
        .await;
        if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            return Err(RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Executor,
                reason: TurnAbortReason::SseDisconnected,
            });
        }
        if let Some(f) = p.ctx.per_flight.as_ref() {
            f.sync_from_per_coord(per_coord);
        }
    }
    Ok(())
}
