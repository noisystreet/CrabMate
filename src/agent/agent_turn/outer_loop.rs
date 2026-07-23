//! 默认 Agent 外层循环：P → R → E，直至结束。
//!
//! 迭代开头守卫、上下文准备、工具表选择与反思分支拆到私有辅助，以降低 `run_agent_outer_loop` 圈复杂度。
//! 单次迭代体为 [`run_outer_loop_single_iteration`]，结束去向由 [`OuterLoopIterationExit`] 显式表达（替代仅依赖 `break`/`continue` 读懂控制流）。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use log::debug;

use crate::check_abort;

use crate::agent::per_coord::PerCoordinator;
use crate::sse::{
    SsePayload, TurnSegmentEndBody, TurnSegmentStartBody, send_sse_control_payload_optional,
};
use crate::types::{Message, USER_CANCELLED_FINISH_REASON, is_intent_gate_ephemeral_system};

use super::errors::{AgentTurnSubPhase, RunAgentTurnError, TurnAbortReason};
use super::execute_tools::{ExecuteToolsBatchOutcome, WebExecuteCtx, per_execute_tools_web};
use super::outer_loop_build_idle::outer_loop_window_has_build_progress_since_last_user;
use super::outer_loop_driver::OuterLoopDriver;
use super::outer_loop_fsm::{OuterLoopIterationExit, OuterLoopIterationPhase, ReflectBranchCtl};
use super::outer_loop_iteration_reduce::outer_loop_iteration_exit_from_reflect_reduce;
use super::outer_loop_reflect::map_reflect_outcome_to_branch_ctl;
use super::params::{OuterLoopPlanCallModelRole, RunLoopParams};
use super::plan::{PerPlanCallModelParams, per_plan_call_model_retrying};
use super::reflect::ReflectOnAssistantOutcome;
use super::reflect::per_reflect_after_assistant;
use super::sub_agent_policy::filter_tool_defs_for_executor_kind;
use super::turn_completion::{
    redundant_tool_names_for_log, task_level_satisfied_allows_early_stop,
    turn_redundant_tools_after_completion_allowed,
};

fn check_shared_turn_budget(p: &RunLoopParams<'_>) -> Result<(), RunAgentTurnError> {
    if let Err(msg) = p
        .turn
        .turn_budget
        .deny_llm_call_if_exhausted(&p.ctx.core.cfg.turn_budget)
    {
        if msg.contains("墙钟") {
            return Err(RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Planner,
                message: msg,
            });
        }
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: msg,
        });
    }
    Ok(())
}

fn outer_loop_iteration_guard(p: &RunLoopParams<'_>) -> Result<(), RunAgentTurnError> {
    let max_iter =
        crate::agent::turn_budget::effective_max_outer_loop_iterations(&p.ctx.core.cfg.turn_budget);
    if p.turn.turn_budget.outer_loop_iterations_exceeded(max_iter) {
        return Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: crate::agent::turn_budget::turn_outer_loop_iterations_limit_user_message(
                max_iter,
            ),
        });
    }
    check_shared_turn_budget(p)?;
    check_abort!(p.ctx.io, AgentTurnSubPhase::Planner);
    Ok(())
}

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
            let mut v = filter_tool_defs_for_executor_kind(
                p.ctx.core.tools_defs,
                p.ctx.core.cfg.as_ref(),
                k,
            );
            if let Some(ref allow) = p.ctx.attach.turn_allowed_tool_names {
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
        full_defs: p.ctx.core.tools_defs,
    }
}

async fn outer_loop_prepare_planner_context(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    {
        let turn = &mut p.turn;
        crate::meta_dialogue::merge_meta_dialogue_into_intent_gate_hint(
            &mut turn.turn_planner_hints,
            turn.messages_buf.as_slice(),
        );
    }
    if let Some(hint) = p.turn.take_intent_turn_gate_hint() {
        p.turn.push_message(Message::system_intent_gate_hint(hint));
    }
    if let Some(ref ltm) = p.ctx.attach.long_term_memory {
        ltm.prepare_messages(
            p.ctx.core.cfg.as_ref(),
            p.ctx.attach.long_term_memory_scope_id.as_deref(),
            p.turn.messages_buffer_mut(),
        );
    }
    p.prepare_turn_messages_for_model(Some(per_coord))
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

async fn outer_loop_reflect_branch(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
) -> Result<ReflectBranchCtl, RunAgentTurnError> {
    let outcome = per_reflect_after_assistant(p, per_coord, finish_reason, msg).await;
    if matches!(outcome, ReflectOnAssistantOutcome::UserCancelled) {
        return Err(RunAgentTurnError::TurnAborted {
            phase: crate::agent::agent_turn::AgentTurnSubPhase::Reflect,
            reason: crate::agent::agent_turn::errors::TurnAbortReason::UserCancelled,
        });
    }
    Ok(map_reflect_outcome_to_branch_ctl(p, per_coord, msg, outcome).await)
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
    let echo_terminal_transcript = render_to_terminal && p.ctx.io.out.is_none();
    let step_executor_constraint = p.turn.turn_planner_hints.step_executor_constraint;
    let exec_outcome = per_execute_tools_web(
        tool_calls,
        per_coord,
        p.turn.messages_buffer_mut(),
        WebExecuteCtx {
            cfg: p.ctx.core.cfg,
            effective_working_dir: p.ctx.core.effective_working_dir,
            workspace_is_set: p.ctx.core.workspace_is_set,
            read_file_turn_cache: p.ctx.attach.read_file_turn_cache.clone(),
            out: p.ctx.io.out,
            tool_running_hook: p.ctx.io.tool_running_hook.clone(),
            clarification_questionnaire_hook: p.ctx.io.clarification_questionnaire_hook.clone(),
            web_tool_ctx: p.ctx.attach.web_tool_ctx,
            cli_tool_ctx: p.ctx.attach.cli_tool_ctx,
            echo_terminal_transcript,
            mcp_turn: p.ctx.attach.mcp_turn.as_ref(),
            workspace_changelist: p.ctx.attach.workspace_changelist.as_ref(),
            request_chrome_trace: p.ctx.obs.request_chrome_trace.clone(),
            step_executor_constraint,
            tools_defs_full: p.ctx.core.tools_defs,
            turn_allow: p
                .ctx
                .attach
                .turn_allowed_tool_names
                .as_ref()
                .map(|a| a.as_ref()),
            long_term_memory: p.ctx.attach.long_term_memory.clone(),
            long_term_memory_scope_id: p.ctx.attach.long_term_memory_scope_id.clone(),
            tracing_chat_turn: p.ctx.obs.tracing_chat_turn.clone(),
            request_audit: p.ctx.obs.request_audit.clone(),
            tool_outcome_recorder: Arc::clone(&p.ctx.obs.process_handles.tool_outcome_recorder),
            handler_lookup: p.ctx.obs.process_handles.handler_lookup.clone(),
            sync_default_sandbox_backend: Arc::clone(
                &p.ctx.obs.process_handles.sync_default_sandbox_backend,
            ),
            readonly_tool_ttl_cache: Arc::clone(&p.ctx.obs.process_handles.readonly_tool_ttl_cache),
            sse_control_mirror: p.ctx.io.sse_control_mirror.clone(),
            sse_encoder: p.ctx.io.sse_encoder.clone(),
        },
    )
    .await;
    if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
        return Err(RunAgentTurnError::TurnAborted {
            phase: AgentTurnSubPhase::Executor,
            reason: TurnAbortReason::SseDisconnected,
        });
    }
    if let Some(f) = p.ctx.attach.per_flight.as_ref() {
        f.sync_from_per_coord(per_coord);
    }
    Ok(())
}

fn completed_goal_with_redundant_tool_calls(p: &mut RunLoopParams<'_>, msg: &Message) -> bool {
    let Some(tool_calls) = msg.tool_calls.as_ref().filter(|calls| !calls.is_empty()) else {
        return false;
    };
    turn_redundant_tools_after_completion_allowed(tool_calls, p.turn.messages())
}

fn drop_redundant_tool_calls_after_active_goal_completed(p: &mut RunLoopParams<'_>, msg: &Message) {
    let popped = p.turn.pop_message();
    let kept = popped
        .as_ref()
        .filter(|m| m.role == "assistant" && m.tool_calls == msg.tool_calls);
    if let Some(m) = kept {
        let has_body =
            crate::types::message_content_as_str(&m.content).is_some_and(|s| !s.trim().is_empty());
        if has_body {
            let mut without_tools = m.clone();
            without_tools.tool_calls = None;
            p.turn.push_assistant_merging_trailing_empty(without_tools);
        }
    } else if let Some(m) = popped {
        p.turn.push_assistant_merging_trailing_empty(m);
    }
}

/// 执行单次 **P → R →（可选）E** 迭代；返回 [`OuterLoopIterationExit`] 供外层循环决策。
async fn run_outer_loop_single_iteration(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    driver: &mut OuterLoopDriver,
) -> Result<OuterLoopIterationExit, RunAgentTurnError> {
    outer_loop_iteration_guard(p)?;
    let iteration_count = p.turn.turn_budget.outer_loop_iterations();
    let plan_model_role = OuterLoopPlanCallModelRole::from_outer_loop_iteration(iteration_count);
    p.apply_outer_loop_plan_call_model_role(plan_model_role);
    let (exec_api_base, exec_api_key) = p.plan_call_executor_endpoint_cloned();

    driver.record_phase(OuterLoopIterationPhase::IterationEnter);
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = driver.phase_str(),
        iteration = iteration_count,
        use_executor_model = p.turn.use_executor_model,
        outer_loop_plan_model_role = plan_model_role.as_trace_str(),
        "outer_loop iteration enter"
    );
    p.turn.sub_phase = AgentTurnSubPhase::Planner;
    if let Some(ref t) = p.ctx.obs.tracing_chat_turn {
        t.on_outer_loop_iteration();
    }

    let render_to_terminal = p.ctx.io.render_to_terminal;
    outer_loop_prepare_planner_context(p, per_coord).await?;

    driver.record_phase(OuterLoopIterationPhase::PrepareContextDone);
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = driver.phase_str(),
        iteration = iteration_count,
        "outer_loop planner context prepared"
    );

    let planner_tools = build_planner_round_tools(p);

    // 非首轮 LLM 调用：通知前端本轮是新的 answer segment，促使其拆分新气泡
    if iteration_count > 1 {
        send_sse_control_payload_optional(
            p.ctx.io.out,
            None,
            SsePayload::TurnSegmentEnd {
                end: TurnSegmentEndBody {
                    segment_id: format!("seg-model-round-{}", iteration_count - 1),
                },
            },
            "outer_loop::prev_answer_segment_end",
            p.ctx.io.sse_encoder.as_ref(),
        )
        .await;
        send_sse_control_payload_optional(
            p.ctx.io.out,
            None,
            SsePayload::TurnSegmentStart {
                start: TurnSegmentStartBody {
                    segment_id: format!("seg-model-round-{iteration_count}"),
                    kind: "answer".to_string(),
                    before_tool_call_id: None,
                },
            },
            "outer_loop::new_answer_segment_start",
            p.ctx.io.sse_encoder.as_ref(),
        )
        .await;
        send_sse_control_payload_optional(
            p.ctx.io.out,
            None,
            SsePayload::AssistantAnswerPhase {
                assistant_answer_phase: true,
            },
            "outer_loop::assistant_answer_phase",
            p.ctx.io.sse_encoder.as_ref(),
        )
        .await;
    }

    let (msg, finish_reason) = per_plan_call_model_retrying(PerPlanCallModelParams {
        llm_backend: p.ctx.core.llm_backend,
        client: p.ctx.core.client,
        api_key: p.ctx.core.api_key,
        cfg: p.ctx.core.cfg.as_ref(),
        tools_defs: planner_tools.as_slice(),
        messages: p.turn.messages(),
        out: p.ctx.io.out,
        render_to_terminal,
        no_stream: p.ctx.io.no_stream,
        cancel: p.ctx.io.cancel,
        plain_terminal_stream: p.ctx.io.plain_terminal_stream,
        tui_llm_stream_scratch: p.ctx.io.tui_llm_stream_scratch.clone(),
        temperature_override: p.turn.temperature_override,
        seed_override: p.turn.seed_override,
        request_chrome_trace: p.ctx.obs.request_chrome_trace.clone(),
        model_override: p.effective_model(),
        executor_api_base: exec_api_base.as_deref(),
        executor_api_key: exec_api_key.as_deref(),
        turn_budget: Some(&p.turn.turn_budget),
    })
    .await
    .map_err(|e| {
        p.turn
            .retain_messages(|m| !is_intent_gate_ephemeral_system(m));
        RunAgentTurnError::from_llm(AgentTurnSubPhase::Planner, e)
    })?;
    p.turn
        .retain_messages(|m| !is_intent_gate_ephemeral_system(m));
    if let Some(f) = p.ctx.attach.per_flight.as_ref() {
        f.awaiting_plan_rewrite_model
            .store(false, Ordering::Relaxed);
    }
    debug!(
        target: "crabmate",
        "模型轮次输出 finish_reason={} message_count_before_push={} assistant_preview={}",
        finish_reason,
        p.turn.messages().len(),
        crate::redact::assistant_message_preview_for_log(&msg)
    );
    p.turn.push_assistant_merging_trailing_empty(msg.clone());

    driver.record_phase(OuterLoopIterationPhase::AfterPlannerModel);
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = driver.phase_str(),
        iteration = iteration_count,
        finish_reason = finish_reason.as_str(),
        "outer_loop assistant pushed"
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        let exit = OuterLoopIterationExit::StopOuterLoop;
        driver.record_iteration_exit(exit);
        return Ok(exit);
    }

    let reflect_ctl = outer_loop_reflect_branch(p, per_coord, finish_reason.as_str(), &msg).await?;
    let reflect_reduce = driver.record_reflect_branch(reflect_ctl);
    driver.record_phase(OuterLoopIterationPhase::ReflectDecided);
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = driver.phase_str(),
        iteration = iteration_count,
        reflect_branch = reflect_reduce.as_str(),
        "outer_loop reflect branch"
    );

    if let Some(exit) = outer_loop_iteration_exit_from_reflect_reduce(reflect_reduce) {
        driver.record_iteration_exit(exit);
        return Ok(exit);
    }

    if completed_goal_with_redundant_tool_calls(p, &msg) {
        let messages = p.turn.messages();
        let goal_preview = crate::types::last_real_user_task_content(messages, false)
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let redundant_tools = msg
            .tool_calls
            .as_ref()
            .map(|calls| redundant_tool_names_for_log(calls.as_slice()))
            .unwrap_or_default();
        tracing::info!(
            target: "crabmate::agent_turn",
            goal_preview = %goal_preview,
            ?redundant_tools,
            "当前用户目标已有完成证据，静默跳过冗余探针/重复 run_command"
        );
        drop_redundant_tool_calls_after_active_goal_completed(p, &msg);
        let exit = OuterLoopIterationExit::StopOuterLoop;
        driver.record_iteration_exit(exit);
        return Ok(exit);
    }

    driver.record_phase(OuterLoopIterationPhase::ToolsExecute);
    tracing::debug!(
        target: "crabmate::agent_turn",
        outer_loop_fsm = "single_agent_outer",
        outer_loop_step = driver.phase_str(),
        iteration = iteration_count,
        "outer_loop tools execute"
    );

    outer_loop_execute_tools_round(p, per_coord, &msg, render_to_terminal).await?;
    if outer_loop_window_has_build_progress_since_last_user(p.turn.messages()) {
        per_coord.reset_outer_loop_build_idle_streak();
    }
    let task_level_early_stop = task_level_satisfied_allows_early_stop(p.turn.messages());
    if task_level_early_stop {
        tracing::info!(
            target: "crabmate::agent_turn",
            "当前用户目标已有完成证据且允许早停，外循环收敛停轮"
        );
    }
    let exit = driver.decide_post_tools_exit(task_level_early_stop);
    driver.record_iteration_exit(exit);
    Ok(exit)
}

pub(crate) async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let mut driver = OuterLoopDriver::new();
    loop {
        p.turn.turn_budget.record_outer_loop_iteration();
        let exit = run_outer_loop_single_iteration(p, per_coord, &mut driver).await?;
        let iteration_count = p.turn.turn_budget.outer_loop_iterations();
        tracing::debug!(
            target: "crabmate::agent_turn",
            outer_loop_fsm = "single_agent_outer",
            outer_loop_iteration_exit = exit.as_trace_str(),
            outer_loop_last_reflect = driver
                .last_reflect
                .as_ref()
                .map(|c| c.as_trace_str())
                .unwrap_or("none"),
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
    use crate::agent::agent_turn::turn_completion::{
        tool_call_is_redundant_after_completion, tool_calls_are_redundant_after_completion,
    };
    use crate::types::{FunctionCall, ToolCall};

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

    fn tc(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: arguments.into(),
            },
        }
    }

    #[test]
    fn completed_gate_marks_readonly_probe_tools_redundant() {
        let calls = vec![
            tc("read_file", r#"{"path":"README.md"}"#),
            tc(
                "run_command",
                r#"{"command":"ls","args":["-lh","target/bin/app"]}"#,
            ),
        ];
        assert!(tool_calls_are_redundant_after_completion(&calls));
    }

    #[test]
    fn completed_gate_marks_run_probe_commands_redundant() {
        assert!(tool_call_is_redundant_after_completion(&tc(
            "run_command",
            r#"{"command":"bash","args":["-c","cd app && ./bin/app --help"]}"#,
        )));
        assert!(tool_call_is_redundant_after_completion(&tc(
            "run_command",
            r#"{"command":"bash","args":["-c","timeout 30 ./bin/app 2>&1"]}"#,
        )));
        assert!(tool_call_is_redundant_after_completion(&tc(
            "run_command",
            r#"{"command":"stat","args":["target/bin/app"]}"#,
        )));
    }

    #[test]
    fn completed_gate_does_not_mark_new_build_command_redundant() {
        let calls = vec![tc(
            "run_command",
            r#"{"command":"make","args":["arch=Linux_Serial","-C","hpcg"]}"#,
        )];
        assert!(!tool_calls_are_redundant_after_completion(&calls));
    }
}
