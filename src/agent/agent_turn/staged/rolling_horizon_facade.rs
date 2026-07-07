//! **滚动视界**外层编排门面：单 Agent / 逻辑双 Agent 共用的
//! **`run_staged_rolling_horizon_outer_loop`**（`turn_fsm` 相位 + 子调用 → advance），以及
//! **`build_single_agent_planner_messages`** / **`build_logical_dual_planner_messages`**。
//!
//! 首轮无工具规划解析后的 ensemble / 优化轮 / 步循环仍在 [`super::run_staged_plan_with_prepared_request`]。
//! 设计对照：`docs/design/per_state_machine_consolidation.md` §3.2。

use crate::agent::per_coord::PerCoordinator;
use crate::types::{
    Message, is_message_excluded_from_llm_context_except_memory, last_staged_step_injection_index,
    message_clone_stripping_reasoning_for_api, staged_step_window_end_exclusive,
};

use super::super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::super::params::RunLoopParams;
use super::super::turn_completion::{
    TurnCompletionDecision, evaluate_turn_staged_rolling_horizon_early_stop,
};
use super::turn_fsm::{
    StagedTurnAdvance, StagedTurnPhase, StagedTurnSubCallOutcome,
    entered_flag_for_next_planner_call, staged_rolling_horizon_apply_advance,
};
use super::{
    StagedPlanRunLabels, prepare_staged_planner_no_tools_request,
    rolling_horizon_advance_reduce::reduce_rolling_horizon_advance,
    rolling_horizon_preflight_reduce::{
        RollingHorizonPreflightAction, RollingHorizonPreflightInput,
        reduce_rolling_horizon_preflight,
    },
    run_staged_plan_with_prepared_request,
    turn_driver::StagedTurnDriver,
};

/// 滚动视界外层循环变体（与 [`advance_staged_turn_after_sub_call`]、`StagedTurnPhase` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagedRollingHorizonKind {
    SingleAgent,
    LogicalDualAgent,
}

impl StagedRollingHorizonKind {
    fn max_rounds_error_message(self, cap: usize) -> String {
        match self {
            Self::SingleAgent => format!(
                "分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                cap
            ),
            Self::LogicalDualAgent => format!(
                "逻辑双Agent分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                cap
            ),
        }
    }

    fn snapshot_rollback_warn_summary(self) -> &'static str {
        match self {
            Self::SingleAgent => "工作区快照回滚失败",
            Self::LogicalDualAgent => "逻辑双Agent快照回滚失败",
        }
    }
}

fn staged_goal_completion_decision_after_step(
    p: &mut RunLoopParams<'_>,
    phase: StagedTurnPhase,
) -> Option<TurnCompletionDecision> {
    if !matches!(phase, StagedTurnPhase::AfterStepExecutionRound) {
        return None;
    }
    Some(evaluate_turn_staged_rolling_horizon_early_stop(
        p.turn.messages(),
        p.turn
            .turn_planner_hints
            .staged_last_completed_step_effective_acceptance
            .as_ref(),
        p.ctx.core.effective_working_dir,
    ))
}

fn staged_rolling_horizon_preflight_exit(
    kind: StagedRollingHorizonKind,
    p: &mut RunLoopParams<'_>,
    phase: StagedTurnPhase,
    staged_rounds: usize,
    max_rounds: usize,
    turn_driver: &mut StagedTurnDriver,
) -> Option<Result<(), RunAgentTurnError>> {
    let early_stop_allow =
        staged_goal_completion_decision_after_step(p, phase).is_some_and(|d| d.is_allow());
    let preflight = RollingHorizonPreflightInput {
        staged_rounds,
        max_rounds,
        phase,
        early_stop_allow,
    };
    let preflight_action = reduce_rolling_horizon_preflight(preflight);
    tracing::debug!(
        target: "crabmate::staged",
        staged_fsm = "rolling_horizon",
        rolling_horizon_preflight = preflight_action.as_str(),
        staged_round = staged_rounds,
        staged_turn_phase = ?phase,
        sub_phase = "planner",
        "staged rolling horizon preflight reduce"
    );
    turn_driver.record_rolling_horizon_preflight(preflight_action);

    match preflight_action {
        RollingHorizonPreflightAction::ContinueIteration => None,
        RollingHorizonPreflightAction::StopMaxRounds => {
            tracing::warn!(
                target: "crabmate::staged",
                staged_fsm = "rolling_horizon",
                rolling_horizon_kind = ?kind,
                staged_round = staged_rounds,
                staged_turn_phase = ?phase,
                sub_phase = "planner",
                "staged rolling horizon exceeded max rounds"
            );
            Some(Err(RunAgentTurnError::Other {
                phase: AgentTurnSubPhase::Planner,
                message: kind.max_rounds_error_message(max_rounds),
            }))
        }
        RollingHorizonPreflightAction::StopEarlyFinish => {
            let decision = staged_goal_completion_decision_after_step(p, phase)
                .expect("early_stop_allow implies decision");
            tracing::info!(
                target: "crabmate::staged",
                staged_fsm = "rolling_horizon",
                rolling_horizon_kind = ?kind,
                staged_round = staged_rounds,
                staged_turn_phase = ?phase,
                sub_phase = "planner",
                turn_completion_decision = decision.as_trace_str(),
                turn_completion_deny_reason = decision.deny_reason(),
                rolling_horizon_via = ?decision.rolling_horizon_via(),
                "staged rolling horizon finished: task-level evidence already satisfies original request"
            );
            Some(Ok(()))
        }
    }
}

/// 单 agent / 逻辑双 agent 共用的 **滚动视界** 外层循环：`turn_fsm` 相位 + 子调用结果 → `StagedTurnAdvance`。
///
/// 见 `docs/design/per_state_machine_consolidation.md` §3.2（分阶段回合编排）；真实转移表在 [`advance_staged_turn_after_sub_call`]。
#[allow(clippy::too_many_arguments)]
async fn run_staged_rolling_horizon_outer_loop<F>(
    kind: StagedRollingHorizonKind,
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    labels: StagedPlanRunLabels,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    make_step_user_message: F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message + Copy,
{
    let mut rewrite_attempts = 0;
    let max_rewrites = p.ctx.core.cfg.turn_budget.full_plan_rewrite_max_attempts;
    let mut phase = StagedTurnPhase::PreStepExecutionRound;
    let mut staged_rounds = 0usize;
    let mut turn_driver = StagedTurnDriver::new();
    const STAGED_SINGLE_STEP_MAX_ROUNDS: usize = 64;
    let snapshot =
        crate::agent::workspace_snapshot::WorkspaceSnapshot::take(p.ctx.core.effective_working_dir);

    loop {
        staged_rounds = staged_rounds.saturating_add(1);
        if let Some(done) = staged_rolling_horizon_preflight_exit(
            kind,
            p,
            phase,
            staged_rounds,
            STAGED_SINGLE_STEP_MAX_ROUNDS,
            &mut turn_driver,
        ) {
            return done;
        }

        turn_driver.record_turn_phase(phase);
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            staged_round = staged_rounds,
            staged_turn_phase = ?phase,
            staged_turn_orchestrator_phase = turn_driver.phase_str(),
            rewrite_attempts = rewrite_attempts,
            sub_phase = "planner",
            "staged rolling horizon iteration enter"
        );

        let req =
            prepare_staged_planner_no_tools_request(p, per_coord, labels.build_planner_messages)
                .await?;
        let entered_from_step_execution_round = entered_flag_for_next_planner_call(phase);
        let res = run_staged_plan_with_prepared_request(
            p,
            per_coord,
            req,
            render_to_terminal,
            echo_terminal_staged,
            entered_from_step_execution_round,
            labels,
            make_step_user_message,
            Some(&mut turn_driver),
        )
        .await;

        let event = match res {
            Ok(o) => StagedTurnSubCallOutcome::Ok(o),
            Err(e) => StagedTurnSubCallOutcome::Err(e),
        };
        let step =
            staged_rolling_horizon_apply_advance(phase, rewrite_attempts, max_rewrites, event);
        rewrite_attempts = step.next_rewrite_attempts;

        let advance_reduce = reduce_rolling_horizon_advance(&step.advance);
        turn_driver.record_rolling_horizon_advance_reduce(advance_reduce);

        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            staged_round = staged_rounds,
            prior_staged_turn_phase = ?phase,
            advance_kind = step.advance_kind,
            rolling_horizon_advance_reduce = advance_reduce.as_str(),
            propagate_public_code = step.propagate_public_code,
            rewrite_attempts = rewrite_attempts,
            sub_phase = "planner",
            "staged rolling horizon advance"
        );

        match advance_reduce {
            super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction::ContinueLoop {
                next_phase,
                restore_workspace_snapshot,
                push_feedback_user,
            } => {
                phase = next_phase;
                if push_feedback_user {
                    if restore_workspace_snapshot {
                        rolling_horizon_try_restore_snapshot(kind, &snapshot);
                    }
                    if let StagedTurnAdvance::Continue {
                        push_feedback_user: Some(u),
                        ..
                    } = &step.advance
                    {
                        p.turn.push_message(u.clone());
                    }
                } else {
                    rolling_horizon_debug_after_step_round(kind, staged_rounds, phase);
                }
                continue;
            }
            super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction::Finish => {
                return Ok(());
            }
            super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction::ReplanExhausted => {
                if let StagedTurnAdvance::ReplanExhausted { phase: ph, message } = step.advance {
                    return Err(RunAgentTurnError::ReplanExhausted { phase: ph, message });
                }
                tracing::error!(
                    target: "crabmate::staged",
                    "advance reduce replan_exhausted without advance payload; returning generic error"
                );
                return Err(RunAgentTurnError::Other {
                    phase: crate::agent::agent_turn::errors::AgentTurnSubPhase::Planner,
                    message: "replan exhausted without advance payload".to_string(),
                });
            }
            super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction::Propagate => {
                if let StagedTurnAdvance::Propagate(e) = step.advance {
                    return Err(e);
                }
                tracing::error!(
                    target: "crabmate::staged",
                    "advance reduce propagate without advance payload; returning generic error"
                );
                return Err(RunAgentTurnError::Other {
                    phase: crate::agent::agent_turn::errors::AgentTurnSubPhase::Planner,
                    message: "propagate without advance payload".to_string(),
                });
            }
        }
    }
}

fn rolling_horizon_try_restore_snapshot(
    kind: StagedRollingHorizonKind,
    snapshot: &Option<crate::agent::workspace_snapshot::WorkspaceSnapshot>,
) {
    let Some(snap) = snapshot else {
        return;
    };
    if let Err(e) = snap.restore() {
        tracing::warn!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            summary = kind.snapshot_rollback_warn_summary(),
            error = %e,
            sub_phase = "planner",
            "workspace snapshot rollback failed"
        );
    } else {
        tracing::info!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            sub_phase = "planner",
            "global replan triggered; workspace snapshot restored"
        );
    }
}

fn rolling_horizon_debug_after_step_round(
    kind: StagedRollingHorizonKind,
    staged_rounds: usize,
    phase: StagedTurnPhase,
) {
    if matches!(phase, StagedTurnPhase::AfterStepExecutionRound) {
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "rolling_horizon",
            rolling_horizon_kind = ?kind,
            staged_round = staged_rounds,
            staged_turn_phase = ?phase,
            outcome = "continue_after_step",
            sub_phase = "planner",
            "step execution round completed; next no-tools planner round"
        );
    }
}

pub(crate) async fn run_staged_plan_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let render_to_terminal = p.ctx.io.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.ctx.io.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "分阶段规划轮模型输出",
        step_injection_log_label: "分步注入 user（完整正文，供排障与日志）",
        build_planner_messages: build_single_agent_planner_messages,
    };

    run_staged_rolling_horizon_outer_loop(
        StagedRollingHorizonKind::SingleAgent,
        p,
        per_coord,
        labels,
        render_to_terminal,
        echo_terminal_staged,
        |body| Message {
            role: "user".to_string(),
            content: Some(body.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    )
    .await
}

pub(crate) async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    let render_to_terminal = p.ctx.io.render_to_terminal;
    let echo_terminal_staged = render_to_terminal && p.ctx.io.out.is_none();

    let labels = StagedPlanRunLabels {
        planning_log_label: "逻辑双agent规划轮输出",
        step_injection_log_label: "逻辑双agent注入执行器user",
        build_planner_messages: build_logical_dual_planner_messages,
    };

    run_staged_rolling_horizon_outer_loop(
        StagedRollingHorizonKind::LogicalDualAgent,
        p,
        per_coord,
        labels,
        render_to_terminal,
        echo_terminal_staged,
        Message::user_staged_orchestration_injection,
    )
    .await
}

pub(crate) fn build_single_agent_planner_messages(
    messages: &[Message],
    plan_system: String,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_message_excluded_from_llm_context_except_memory(m))
        .map(|m| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

pub(crate) fn build_logical_dual_planner_messages(
    messages: &[Message],
    plan_system: String,
    preserve_reasoning_on_assistant_tool_calls: bool,
    preserve_deepseek_thinking_reasoning_roundtrip: bool,
) -> Vec<Message> {
    let last_step_idx = last_staged_step_injection_index(messages);
    let last_step_end = last_step_idx.map(|i| staged_step_window_end_exclusive(messages, i));

    let mut out: Vec<Message> = messages
        .iter()
        .enumerate()
        .filter(|(idx, m)| {
            if is_message_excluded_from_llm_context_except_memory(m) {
                return false;
            }
            // 逻辑双 agent：全局剥离 tool，但保留**最近一分步窗口**内观测，供步后重规划。
            if m.role == "tool" {
                if let (Some(step_idx), Some(end)) = (last_step_idx, last_step_end) {
                    return *idx > step_idx && *idx < end;
                }
                return false;
            }
            if m.role == "assistant" {
                return crate::types::message_content_as_str(&m.content)
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
            }
            true
        })
        .map(|(_, m)| {
            message_clone_stripping_reasoning_for_api(
                m,
                preserve_reasoning_on_assistant_tool_calls,
                preserve_deepseek_thinking_reasoning_roundtrip,
            )
        })
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

#[cfg(test)]
mod staged_rolling_horizon_kind_tests {
    use super::StagedRollingHorizonKind;

    #[test]
    fn max_rounds_error_messages_distinct_by_variant() {
        let a = StagedRollingHorizonKind::SingleAgent.max_rounds_error_message(64);
        let b = StagedRollingHorizonKind::LogicalDualAgent.max_rounds_error_message(64);
        assert_ne!(a, b);
        assert!(a.contains("分阶段单步"), "{a}");
        assert!(b.contains("逻辑双Agent"), "{b}");
    }
}
