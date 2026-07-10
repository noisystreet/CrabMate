//! 分阶段回合**顶层**编排相位（`docs/design/per_state_machine_consolidation.md` §3.2）。
//!
//! 子 FSM（`turn_fsm`、`plan_pipeline_schedule`、`prepared_parse_fsm`、`step_loop` 等）仍各自维护细粒度转移；
//! 本模块提供**统一词汇表**与 `tracing` 字段 **`staged_turn_orchestrator_phase`**，便于排障时对照设计稿，
//! 而不强行把全部子状态合并为单一运行时变量。

use super::orchestrator::StagedRoundOrchestratorPhase;
use super::plan_pipeline_schedule::{PreparedPostParseSchedule, StagedFullPipelinePhase};
use super::prepared_parse_fsm::{PreparedPlannerRoute, PreparedRouteReduceAction};
use super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction;
use super::rolling_horizon_preflight_reduce::RollingHorizonPreflightAction;
use super::staged_parse_terminal::StagedParseTerminalRoute;
use super::step_loop::{StepIterationReduceAction, StepPatchRecoverReduceAction};
use super::turn_fsm::StagedTurnPhase;

/// 设计稿 §3.2「分阶段回合 FSM」顶层相位（与 `StagedTurnPhase` 滚动视界、
/// `StagedRoundOrchestratorPhase` 定稿后 SSE 等**正交**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedTurnOrchestratorPhase {
    /// 无工具规划轮（含滚动视界重入、补丁规划前的准备）。
    PrePlan,
    /// 已得合法 `agent_reply_plan`，首轮后管线（ensemble / 优化 / NL）或待发 `staged_plan_started`。
    PlanReady,
    /// 分步执行队列内（含单步 outer_loop）。
    StepRunning,
    /// `patch_planner` 模式下步失败后的无工具重规划。
    PatchReplanner,
    /// 规划解析失败等路径降级到 `run_agent_outer_loop`。
    DegradedToOuterLoop,
    /// 本分阶段回合正常结束（`no_task`、静默收敛、队列跑完等）。
    Done,
}

crate::impl_as_str!(StagedTurnOrchestratorPhase, {
    Self::PrePlan => "pre_plan",
    Self::PlanReady => "plan_ready",
    Self::StepRunning => "step_running",
    Self::PatchReplanner => "patch_replanner",
    Self::DegradedToOuterLoop => "degraded_to_outer_loop",
    Self::Done => "done",
});

/// 顶层编排 **事件**（`StagedTurnDriver` 单表入口；与 `orchestrator_phase_for_*` 对齐）。
#[derive(Debug, Clone)]
pub(crate) enum StagedTurnOrchestratorEvent<'a> {
    TurnPhase(StagedTurnPhase),
    ParseTerminal(&'a StagedParseTerminalRoute),
    PreparedRouteReduce(PreparedRouteReduceAction),
    StepIterationReduce(StepIterationReduceAction),
    RollingHorizonPreflight(RollingHorizonPreflightAction),
    RollingHorizonAdvance(RollingHorizonAdvanceReduceAction),
    StepPatchRecoverReduce(&'a StepPatchRecoverReduceAction),
    PostParseSchedule(PreparedPostParseSchedule),
    FullPipelinePhase(StagedFullPipelinePhase),
    RoundOrchestrator(StagedRoundOrchestratorPhase),
    StepsLoopTrace(&'a str),
    StepsLoopEarlyFinishSuccess,
}

/// 单表转移结果：`Unchanged` 表示保留当前 `StagedTurnDriver.phase`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedTurnOrchestratorStepOutcome {
    Set(StagedTurnOrchestratorPhase),
    Unchanged,
}

/// **(事件) → 下一顶层相位** 权威表；`record_*` 与金样 `orchestrator_phase_for_*` 均经此函数。
pub(crate) fn staged_turn_orchestrator_step(
    event: StagedTurnOrchestratorEvent<'_>,
) -> StagedTurnOrchestratorStepOutcome {
    match event {
        StagedTurnOrchestratorEvent::TurnPhase(phase) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_turn_phase(phase))
        }
        StagedTurnOrchestratorEvent::ParseTerminal(terminal) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_prepared_route(
                &terminal.to_prepared_planner_route(),
            ))
        }
        StagedTurnOrchestratorEvent::PreparedRouteReduce(action) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_prepared_route_reduce(
                action,
            ))
        }
        StagedTurnOrchestratorEvent::StepIterationReduce(action) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_step_iteration_reduce(
                action,
            ))
        }
        StagedTurnOrchestratorEvent::RollingHorizonPreflight(
            RollingHorizonPreflightAction::ContinueIteration,
        ) => StagedTurnOrchestratorStepOutcome::Unchanged,
        StagedTurnOrchestratorEvent::RollingHorizonPreflight(action) => {
            StagedTurnOrchestratorStepOutcome::Set(
                orchestrator_phase_for_rolling_horizon_preflight(action),
            )
        }
        StagedTurnOrchestratorEvent::RollingHorizonAdvance(
            RollingHorizonAdvanceReduceAction::ContinueLoop { next_phase, .. },
        ) => StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_turn_phase(next_phase)),
        StagedTurnOrchestratorEvent::RollingHorizonAdvance(
            RollingHorizonAdvanceReduceAction::Finish,
        ) => StagedTurnOrchestratorStepOutcome::Set(StagedTurnOrchestratorPhase::Done),
        StagedTurnOrchestratorEvent::RollingHorizonAdvance(
            RollingHorizonAdvanceReduceAction::ReplanExhausted
            | RollingHorizonAdvanceReduceAction::Propagate,
        ) => StagedTurnOrchestratorStepOutcome::Unchanged,
        StagedTurnOrchestratorEvent::StepPatchRecoverReduce(StepPatchRecoverReduceAction::Run(
            _,
        )) => StagedTurnOrchestratorStepOutcome::Set(StagedTurnOrchestratorPhase::PatchReplanner),
        StagedTurnOrchestratorEvent::StepPatchRecoverReduce(_) => {
            StagedTurnOrchestratorStepOutcome::Unchanged
        }
        StagedTurnOrchestratorEvent::PostParseSchedule(schedule) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_post_parse_schedule(
                schedule,
            ))
        }
        StagedTurnOrchestratorEvent::FullPipelinePhase(phase) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_full_pipeline(phase))
        }
        StagedTurnOrchestratorEvent::RoundOrchestrator(phase) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_round_orchestrator(phase))
        }
        StagedTurnOrchestratorEvent::StepsLoopTrace(label) => {
            StagedTurnOrchestratorStepOutcome::Set(orchestrator_phase_for_steps_loop_trace(label))
        }
        StagedTurnOrchestratorEvent::StepsLoopEarlyFinishSuccess => {
            StagedTurnOrchestratorStepOutcome::Set(StagedTurnOrchestratorPhase::Done)
        }
    }
}

/// 首轮后 post-parse 调度 → 顶层（`no_task` 路径视为降级外循环）。
pub(crate) fn orchestrator_phase_for_post_parse_schedule(
    schedule: PreparedPostParseSchedule,
) -> StagedTurnOrchestratorPhase {
    match schedule {
        PreparedPostParseSchedule::NoTaskThenOuter => {
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        }
        PreparedPostParseSchedule::FullPipelineThenSteps => StagedTurnOrchestratorPhase::PlanReady,
    }
}

/// 首轮规划解析路由 → 顶层相位（`run_staged_plan_with_prepared_request`）。
pub(crate) fn orchestrator_phase_for_prepared_route(
    route: &PreparedPlannerRoute,
) -> StagedTurnOrchestratorPhase {
    match route {
        PreparedPlannerRoute::QuietFinish | PreparedPlannerRoute::FinishWithDirectPlannerAnswer => {
            StagedTurnOrchestratorPhase::Done
        }
        PreparedPlannerRoute::DegradeToOuterLoop => {
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        }
        PreparedPlannerRoute::ContinueWithPlan { .. } => StagedTurnOrchestratorPhase::PlanReady,
    }
}

/// 滚动视界外层 `StagedTurnPhase` → 顶层（步后重入规划仍属 `PrePlan`）。
pub(crate) fn orchestrator_phase_for_turn_phase(
    phase: StagedTurnPhase,
) -> StagedTurnOrchestratorPhase {
    match phase {
        StagedTurnPhase::PreStepExecutionRound | StagedTurnPhase::AfterStepExecutionRound => {
            StagedTurnOrchestratorPhase::PrePlan
        }
    }
}

/// 首轮后管线各段均属「规划已定稿、尚未或正在进入分步」。
pub(crate) fn orchestrator_phase_for_full_pipeline(
    _phase: StagedFullPipelinePhase,
) -> StagedTurnOrchestratorPhase {
    StagedTurnOrchestratorPhase::PlanReady
}

/// 首轮规划 parse reduce → 顶层相位。
pub(crate) fn orchestrator_phase_for_prepared_route_reduce(
    action: PreparedRouteReduceAction,
) -> StagedTurnOrchestratorPhase {
    match action {
        PreparedRouteReduceAction::FinishQuiet
        | PreparedRouteReduceAction::FinishWithAssistantOnly => StagedTurnOrchestratorPhase::Done,
        PreparedRouteReduceAction::DegradeToOuterLoop => {
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        }
        PreparedRouteReduceAction::ContinuePostParse => StagedTurnOrchestratorPhase::PlanReady,
    }
}

/// 步后 outer_loop reduce → 顶层相位。
pub(crate) fn orchestrator_phase_for_step_iteration_reduce(
    action: StepIterationReduceAction,
) -> StagedTurnOrchestratorPhase {
    match action {
        StepIterationReduceAction::ToolFailurePatch => StagedTurnOrchestratorPhase::PatchReplanner,
        StepIterationReduceAction::ExecOrVerifyFailed
        | StepIterationReduceAction::Cancelled
        | StepIterationReduceAction::EmitSuccessAdvance => StagedTurnOrchestratorPhase::StepRunning,
    }
}

/// 滚动视界 preflight reduce → 顶层相位（Continue 时调用方保留当前相位）。
pub(crate) fn orchestrator_phase_for_rolling_horizon_preflight(
    action: RollingHorizonPreflightAction,
) -> StagedTurnOrchestratorPhase {
    match action {
        RollingHorizonPreflightAction::ContinueIteration => StagedTurnOrchestratorPhase::PrePlan,
        RollingHorizonPreflightAction::StopMaxRounds
        | RollingHorizonPreflightAction::StopEarlyFinish => StagedTurnOrchestratorPhase::Done,
    }
}

/// 定稿 SSE 后进入步队列。
pub(crate) fn orchestrator_phase_for_round_orchestrator(
    _phase: StagedRoundOrchestratorPhase,
) -> StagedTurnOrchestratorPhase {
    StagedTurnOrchestratorPhase::StepRunning
}

/// `steps_loop` 内细粒度字符串相位 → 顶层（用于统一 `tracing` 字段）。
pub(crate) fn orchestrator_phase_for_steps_loop_trace(
    steps_loop_phase: &str,
) -> StagedTurnOrchestratorPhase {
    match steps_loop_phase {
        "steps_executing_enter"
        | "step_running"
        | "cancelled_before_step"
        | "cancelled_after_outer_ok" => StagedTurnOrchestratorPhase::StepRunning,
        "patch_replanner" | "patch_replanner_attempt" | "patch_replanner_tool_failure" => {
            StagedTurnOrchestratorPhase::PatchReplanner
        }
        "send_plan_finished" => StagedTurnOrchestratorPhase::Done,
        _ => StagedTurnOrchestratorPhase::StepRunning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::staged::{
        prepared_parse_fsm::PreparedRouteReduceAction,
        rolling_horizon_preflight_reduce::RollingHorizonPreflightAction,
        step_loop::StepIterationReduceAction,
    };
    use crate::agent::plan_artifact::AgentReplyPlanV1;

    #[test]
    fn prepared_route_reduce_maps_to_top_level() {
        assert_eq!(
            orchestrator_phase_for_prepared_route_reduce(PreparedRouteReduceAction::FinishQuiet),
            StagedTurnOrchestratorPhase::Done
        );
        assert_eq!(
            orchestrator_phase_for_prepared_route_reduce(
                PreparedRouteReduceAction::DegradeToOuterLoop
            ),
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        );
        assert_eq!(
            orchestrator_phase_for_prepared_route_reduce(
                PreparedRouteReduceAction::ContinuePostParse
            ),
            StagedTurnOrchestratorPhase::PlanReady
        );
    }

    #[test]
    fn step_iteration_reduce_maps_to_top_level() {
        assert_eq!(
            orchestrator_phase_for_step_iteration_reduce(
                StepIterationReduceAction::ToolFailurePatch
            ),
            StagedTurnOrchestratorPhase::PatchReplanner
        );
        assert_eq!(
            orchestrator_phase_for_step_iteration_reduce(
                StepIterationReduceAction::EmitSuccessAdvance
            ),
            StagedTurnOrchestratorPhase::StepRunning
        );
    }

    #[test]
    fn rolling_horizon_preflight_maps_to_top_level() {
        assert_eq!(
            orchestrator_phase_for_rolling_horizon_preflight(
                RollingHorizonPreflightAction::StopEarlyFinish
            ),
            StagedTurnOrchestratorPhase::Done
        );
        assert_eq!(
            orchestrator_phase_for_rolling_horizon_preflight(
                RollingHorizonPreflightAction::ContinueIteration
            ),
            StagedTurnOrchestratorPhase::PrePlan
        );
    }

    #[test]
    fn post_parse_schedule_maps_to_top_level() {
        assert_eq!(
            orchestrator_phase_for_post_parse_schedule(PreparedPostParseSchedule::NoTaskThenOuter),
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        );
        assert_eq!(
            orchestrator_phase_for_post_parse_schedule(
                PreparedPostParseSchedule::FullPipelineThenSteps
            ),
            StagedTurnOrchestratorPhase::PlanReady
        );
    }

    #[test]
    fn patch_replanner_trace_maps_to_patch_phase() {
        assert_eq!(
            orchestrator_phase_for_steps_loop_trace("patch_replanner_attempt"),
            StagedTurnOrchestratorPhase::PatchReplanner
        );
    }

    #[test]
    fn prepared_route_maps_to_top_level() {
        assert_eq!(
            orchestrator_phase_for_prepared_route(&PreparedPlannerRoute::DegradeToOuterLoop),
            StagedTurnOrchestratorPhase::DegradedToOuterLoop
        );
        assert_eq!(
            orchestrator_phase_for_prepared_route(&PreparedPlannerRoute::QuietFinish),
            StagedTurnOrchestratorPhase::Done
        );
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![],
            no_task: true,
        };
        assert_eq!(
            orchestrator_phase_for_prepared_route(&PreparedPlannerRoute::ContinueWithPlan { plan }),
            StagedTurnOrchestratorPhase::PlanReady
        );
    }

    #[test]
    fn turn_phase_always_pre_plan_at_top_level() {
        assert_eq!(
            orchestrator_phase_for_turn_phase(StagedTurnPhase::PreStepExecutionRound),
            StagedTurnOrchestratorPhase::PrePlan
        );
        assert_eq!(
            orchestrator_phase_for_turn_phase(StagedTurnPhase::AfterStepExecutionRound),
            StagedTurnOrchestratorPhase::PrePlan
        );
    }

    #[test]
    fn steps_loop_trace_mapping() {
        assert_eq!(
            orchestrator_phase_for_steps_loop_trace("send_plan_finished"),
            StagedTurnOrchestratorPhase::Done
        );
        assert_eq!(
            orchestrator_phase_for_steps_loop_trace("step_running"),
            StagedTurnOrchestratorPhase::StepRunning
        );
    }

    #[test]
    fn single_table_preflight_continue_is_unchanged() {
        assert_eq!(
            staged_turn_orchestrator_step(StagedTurnOrchestratorEvent::RollingHorizonPreflight(
                RollingHorizonPreflightAction::ContinueIteration
            )),
            StagedTurnOrchestratorStepOutcome::Unchanged
        );
    }

    #[test]
    fn single_table_matches_legacy_wrappers() {
        assert_eq!(
            staged_turn_orchestrator_step(StagedTurnOrchestratorEvent::PreparedRouteReduce(
                PreparedRouteReduceAction::ContinuePostParse
            )),
            StagedTurnOrchestratorStepOutcome::Set(StagedTurnOrchestratorPhase::PlanReady)
        );
        assert_eq!(
            staged_turn_orchestrator_step(StagedTurnOrchestratorEvent::StepIterationReduce(
                StepIterationReduceAction::ToolFailurePatch
            )),
            StagedTurnOrchestratorStepOutcome::Set(StagedTurnOrchestratorPhase::PatchReplanner)
        );
    }
}
