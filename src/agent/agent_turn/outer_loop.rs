//! 默认 Agent 外层循环：P → R → E，直至结束。
//!
//! 迭代开头守卫、上下文准备、工具表选择与反思分支拆到私有辅助，以降低 `run_agent_outer_loop` 圈复杂度。
//! 单次迭代体为 [`run_outer_loop_single_iteration`]，结束去向由 [`OuterLoopIterationExit`] 显式表达（替代仅依赖 `break`/`continue` 读懂控制流）。

use std::sync::Arc;
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
use super::params::{OuterLoopPlanCallModelRole, RunLoopParams};
use super::plan::{PerPlanCallModelParams, per_plan_call_model_retrying};
use super::reflect::{ReflectOnAssistantOutcome, per_reflect_after_assistant};
use super::sub_agent_policy::filter_tool_defs_for_executor_kind;

const MAX_OUTER_LOOP_ITERATIONS_SAFETY: u32 = 500;

/// 步级子代理约束时可能需 `Vec` 持有过滤后的工具定义；否则用全量 `tools_defs` 切片。
struct PlannerRoundTools<'a> {
    owned_filtered: Option<Vec<crate::types::Tool>>,
    full_defs: &'a [crate::types::Tool],
}

impl<'a> PlannerRoundTools<'a> {
    fn as_slice(&self) -> &[crate::types::Tool] {
        self.owned_filtered.as_deref().unwrap_or(self.full_defs)
    }
}

fn build_planner_round_tools<'a>(p: &RunLoopParams<'a>) -> PlannerRoundTools<'a> {
    let owned_filtered = match p.turn.turn_planner_hints.step_executor_constraint {
        Some(k) => {
            let mut v = filter_tool_defs_for_executor_kind(p.ctx.tools_defs, p.ctx.cfg.as_ref(), k);
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
            Some(v)
        }
        None => None,
    };
    PlannerRoundTools {
        owned_filtered,
        full_defs: p.ctx.tools_defs,
    }
}

fn outer_loop_iteration_guard(
    iteration_count: u32,
    p: &RunLoopParams<'_>,
    start_time: std::time::Instant,
) -> Result<(), RunAgentTurnError> {
    if iteration_count > MAX_OUTER_LOOP_ITERATIONS_SAFETY {
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: format!(
                "达到外层循环安全上限（{} 轮），已中止以避免重复工具调用死循环",
                MAX_OUTER_LOOP_ITERATIONS_SAFETY
            ),
        });
    }
    if crate::agent::turn_budget::turn_wall_clock_exceeded(
        p.ctx.cfg.turn_budget.max_turn_duration_seconds,
        start_time.elapsed().as_secs(),
    ) {
        return Err(RunAgentTurnError::TimeLimitExhausted {
            phase: AgentTurnSubPhase::Planner,
            message: crate::agent::turn_budget::turn_wall_clock_limit_user_message(
                p.ctx.cfg.turn_budget.max_turn_duration_seconds,
            ),
        });
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
    Ok(())
}

async fn outer_loop_prepare_planner_context(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    if let Some(hint) = p.turn.take_intent_turn_gate_hint() {
        p.turn.push_message(Message::system_intent_gate_hint(hint));
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
        p.ctx.workspace_changelist.as_ref().map(|a| a.as_ref()),
        crate::agent::context_window::PrepareMessagesForModelHooks {
            per_coord_layer_cache: Some(per_coord),
            run_loop_messages_revision: Some(&mut p.turn.messages_revision),
        },
    )
    .await
    .map_err(|e| {
        p.turn
            .retain_messages(|m| !is_intent_gate_ephemeral_system(m));
        RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: e.to_string(),
        }
    })
}

#[derive(Debug)]
enum ReflectBranchCtl {
    /// 结束外层循环（正常停轮或规划重写耗尽已处理 SSE）。
    BreakOuter,
    /// `continue 'outer`（规划重写）。
    ContinueOuter,
    /// 进入工具执行阶段。
    ProceedToTools,
}

impl ReflectBranchCtl {
    fn as_trace_str(&self) -> &'static str {
        match self {
            Self::BreakOuter => "break_outer",
            Self::ContinueOuter => "continue_outer",
            Self::ProceedToTools => "proceed_to_tools",
        }
    }
}

async fn outer_loop_reflect_branch(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
) -> ReflectBranchCtl {
    match per_reflect_after_assistant(p, per_coord, finish_reason, msg).await {
        ReflectOnAssistantOutcome::StopTurn => {
            if let Some(f) = p.ctx.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
            }
            ReflectBranchCtl::BreakOuter
        }
        ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
            if let Some(f) = p.ctx.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
                f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
            }
            ReflectBranchCtl::ContinueOuter
        }
        ReflectOnAssistantOutcome::ProceedToExecuteTools => {
            if let Some(f) = p.ctx.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
            }
            ReflectBranchCtl::ProceedToTools
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
            ReflectBranchCtl::BreakOuter
        }
    }
}

async fn outer_loop_execute_tools_round(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    msg: &Message,
    render_to_terminal: bool,
) -> Result<(), RunAgentTurnError> {
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
            tool_running_hook: p.ctx.tool_running_hook.clone(),
            web_tool_ctx: p.ctx.web_tool_ctx,
            cli_tool_ctx: p.ctx.cli_tool_ctx,
            echo_terminal_transcript,
            mcp_session: p.ctx.mcp_session.as_ref(),
            workspace_changelist: p.ctx.workspace_changelist.as_ref(),
            request_chrome_trace: p.ctx.request_chrome_trace.clone(),
            step_executor_constraint: p.turn.turn_planner_hints.step_executor_constraint,
            tools_defs_full: p.ctx.tools_defs,
            turn_allow: p.ctx.turn_allowed_tool_names.as_ref().map(|a| a.as_ref()),
            long_term_memory: p.ctx.long_term_memory.clone(),
            long_term_memory_scope_id: p.ctx.long_term_memory_scope_id.clone(),
            tracing_chat_turn: p.ctx.tracing_chat_turn.clone(),
            request_audit: p.ctx.request_audit.clone(),
            tool_outcome_recorder: Arc::clone(&p.ctx.process_handles.tool_outcome_recorder),
            handler_lookup: p.ctx.process_handles.handler_lookup.clone(),
            sync_default_sandbox_backend: Arc::clone(
                &p.ctx.process_handles.sync_default_sandbox_backend,
            ),
            readonly_tool_ttl_cache: Arc::clone(&p.ctx.process_handles.readonly_tool_ttl_cache),
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
    Ok(())
}

/// 单 Agent 外循环内一次迭代的**粗粒度**阶段（与 `AgentTurnSubPhase` 正交，仅用于 `tracing` 排障）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OuterLoopIterationPhase {
    /// 通过迭代守卫后、准备 planner 上下文前（[`OuterLoopPlanCallModelRole`] 已应用到 `use_executor_model`）。
    IterationEnter,
    /// `prepare_messages_for_model` 等准备完成，即将 `per_plan_call_model_retrying`。
    PrepareContextDone,
    /// 已 `push` assistant，即将反思或（若 `ProceedToTools`）工具轮。
    AfterPlannerModel,
    /// 反思分支已决（不进入工具 / 重开一轮 / 去工具）。
    ReflectDecided,
    /// `per_execute_tools_web` 工具批。
    ToolsExecute,
}

impl OuterLoopIterationPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::IterationEnter => "iteration_enter",
            Self::PrepareContextDone => "prepare_context_done",
            Self::AfterPlannerModel => "after_planner_model",
            Self::ReflectDecided => "reflect_decided",
            Self::ToolsExecute => "tools_execute",
        }
    }
}

/// 单次外层迭代结束后的显式去向（替代隐式 `break` / `continue`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OuterLoopIterationExit {
    /// 进入下一轮 `per_plan_call_model_retrying`（含规划重写 `continue` 语义）。
    ContinueNextIteration,
    /// 结束 `run_agent_outer_loop`（正常停轮、取消、`BreakOuter` 等）。
    StopOuterLoop,
}

impl OuterLoopIterationExit {
    fn as_trace_str(self) -> &'static str {
        match self {
            Self::ContinueNextIteration => "continue_next_iteration",
            Self::StopOuterLoop => "stop_outer_loop",
        }
    }
}

/// 执行单次 **P → R →（可选）E** 迭代；返回 [`OuterLoopIterationExit`] 供外层循环决策。
async fn run_outer_loop_single_iteration(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    iteration_count: u32,
    start_time: std::time::Instant,
) -> Result<OuterLoopIterationExit, RunAgentTurnError> {
    outer_loop_iteration_guard(iteration_count, p, start_time)?;
    let plan_model_role = OuterLoopPlanCallModelRole::from_outer_loop_iteration(iteration_count);
    p.apply_outer_loop_plan_call_model_role(plan_model_role);
    let (exec_api_base, exec_api_key) = p.plan_call_executor_endpoint_cloned();

    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = OuterLoopIterationPhase::IterationEnter.as_str(),
        iteration = iteration_count,
        use_executor_model = p.turn.use_executor_model,
        outer_loop_plan_model_role = plan_model_role.as_trace_str(),
        "outer_loop iteration enter"
    );
    p.turn.sub_phase = AgentTurnSubPhase::Planner;
    if let Some(ref t) = p.ctx.tracing_chat_turn {
        t.on_outer_loop_iteration();
    }

    let render_to_terminal = p.ctx.render_to_terminal;
    outer_loop_prepare_planner_context(p, per_coord).await?;

    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = OuterLoopIterationPhase::PrepareContextDone.as_str(),
        iteration = iteration_count,
        "outer_loop planner context prepared"
    );

    let planner_tools = build_planner_round_tools(p);
    let (msg, finish_reason) = per_plan_call_model_retrying(PerPlanCallModelParams {
        llm_backend: p.ctx.llm_backend,
        client: p.ctx.client,
        api_key: p.ctx.api_key,
        cfg: p.ctx.cfg.as_ref(),
        tools_defs: planner_tools.as_slice(),
        messages: p.turn.messages,
        out: p.ctx.out,
        render_to_terminal,
        no_stream: p.ctx.no_stream,
        cancel: p.ctx.cancel,
        plain_terminal_stream: p.ctx.plain_terminal_stream,
        tui_llm_stream_scratch: p.ctx.tui_llm_stream_scratch.clone(),
        temperature_override: p.turn.temperature_override,
        seed_override: p.turn.seed_override,
        request_chrome_trace: p.ctx.request_chrome_trace.clone(),
        model_override: p.effective_model(),
        executor_api_base: exec_api_base.as_deref(),
        executor_api_key: exec_api_key.as_deref(),
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
    p.turn.push_assistant_merging_trailing_empty(msg.clone());

    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = OuterLoopIterationPhase::AfterPlannerModel.as_str(),
        iteration = iteration_count,
        finish_reason = finish_reason.as_str(),
        "outer_loop assistant pushed"
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(OuterLoopIterationExit::StopOuterLoop);
    }

    let reflect_ctl = outer_loop_reflect_branch(p, per_coord, finish_reason.as_str(), &msg).await;
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = OuterLoopIterationPhase::ReflectDecided.as_str(),
        iteration = iteration_count,
        reflect_branch = reflect_ctl.as_trace_str(),
        "outer_loop reflect branch"
    );

    match reflect_ctl {
        ReflectBranchCtl::BreakOuter => {
            return Ok(OuterLoopIterationExit::StopOuterLoop);
        }
        ReflectBranchCtl::ContinueOuter => {
            return Ok(OuterLoopIterationExit::ContinueNextIteration);
        }
        ReflectBranchCtl::ProceedToTools => {}
    }

    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = OuterLoopIterationPhase::ToolsExecute.as_str(),
        iteration = iteration_count,
        "outer_loop tools execute"
    );

    outer_loop_execute_tools_round(p, per_coord, &msg, render_to_terminal).await?;
    Ok(OuterLoopIterationExit::ContinueNextIteration)
}

pub(crate) async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let start_time = std::time::Instant::now();
    let mut iteration_count: u32 = 0;
    loop {
        iteration_count = iteration_count.saturating_add(1);
        let exit =
            run_outer_loop_single_iteration(p, per_coord, iteration_count, start_time).await?;
        tracing::debug!(
            target: "crabmate::agent_turn",
            outer_loop_fsm = "single_agent_outer",
            outer_loop_iteration_exit = exit.as_trace_str(),
            iteration = iteration_count,
            "outer_loop iteration exit decision"
        );
        match exit {
            OuterLoopIterationExit::ContinueNextIteration => {}
            OuterLoopIterationExit::StopOuterLoop => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::OuterLoopIterationExit;

    #[test]
    fn outer_loop_iteration_exit_trace_str_stable() {
        assert_eq!(
            OuterLoopIterationExit::ContinueNextIteration.as_trace_str(),
            "continue_next_iteration"
        );
        assert_eq!(
            OuterLoopIterationExit::StopOuterLoop.as_trace_str(),
            "stop_outer_loop"
        );
    }
}
