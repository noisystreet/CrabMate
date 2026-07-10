//! 分阶段回合 **运行时 driver**（`StagedTurnOrchestratorPhase` 权威 + 步后早停决策）。
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2。
//!
//! 所有 `record_*` 经 **`staged_turn_orchestrator_step`** 单表更新相位。

use super::super::params::RunLoopParams;
use super::super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
};
use super::super::turn_completion::evaluate_turn_staged_rolling_horizon_early_stop;
use super::StagedPlanRunOutcome;
use super::orchestrator::StagedRoundOrchestratorPhase;
use super::plan_pipeline_schedule::{PreparedPostParseSchedule, StagedFullPipelinePhase};
use super::prepared_parse_fsm::PreparedRouteReduceAction;
use super::rolling_horizon_advance_reduce::RollingHorizonAdvanceReduceAction;
use super::rolling_horizon_preflight_reduce::RollingHorizonPreflightAction;
use super::staged_parse_terminal::StagedParseTerminalRoute;
use super::step_loop::{StepIterationReduceAction, StepPatchRecoverReduceAction};
use super::turn_fsm::StagedTurnPhase;
use super::turn_orchestrator_fsm::{
    StagedTurnOrchestratorEvent, StagedTurnOrchestratorPhase, StagedTurnOrchestratorStepOutcome,
    staged_turn_orchestrator_step,
};

/// 滚动视界外层持有的运行时顶层相位（与 **`tracing`** `staged_turn_orchestrator_phase` 对齐）。
#[derive(Debug, Clone)]
pub(crate) struct StagedTurnDriver {
    pub(crate) phase: StagedTurnOrchestratorPhase,
}

impl StagedTurnDriver {
    pub(crate) fn new() -> Self {
        Self {
            phase: StagedTurnOrchestratorPhase::PrePlan,
        }
    }

    fn apply_event(&mut self, event: StagedTurnOrchestratorEvent<'_>) {
        match staged_turn_orchestrator_step(event) {
            StagedTurnOrchestratorStepOutcome::Set(phase) => self.phase = phase,
            StagedTurnOrchestratorStepOutcome::Unchanged => {}
        }
    }

    pub(crate) fn record_turn_phase(&mut self, turn_phase: StagedTurnPhase) {
        self.apply_event(StagedTurnOrchestratorEvent::TurnPhase(turn_phase));
    }

    pub(crate) fn record_parse_terminal(&mut self, terminal: &StagedParseTerminalRoute) {
        self.apply_event(StagedTurnOrchestratorEvent::ParseTerminal(terminal));
    }

    pub(crate) fn record_prepared_route_reduce(&mut self, action: PreparedRouteReduceAction) {
        self.apply_event(StagedTurnOrchestratorEvent::PreparedRouteReduce(action));
    }

    pub(crate) fn record_step_iteration_reduce(&mut self, action: StepIterationReduceAction) {
        self.apply_event(StagedTurnOrchestratorEvent::StepIterationReduce(action));
    }

    pub(crate) fn record_rolling_horizon_preflight(
        &mut self,
        action: RollingHorizonPreflightAction,
    ) {
        self.apply_event(StagedTurnOrchestratorEvent::RollingHorizonPreflight(action));
    }

    pub(crate) fn record_rolling_horizon_advance_reduce(
        &mut self,
        action: RollingHorizonAdvanceReduceAction,
    ) {
        self.apply_event(StagedTurnOrchestratorEvent::RollingHorizonAdvance(action));
    }

    pub(crate) fn record_step_patch_recover_reduce(
        &mut self,
        action: &StepPatchRecoverReduceAction,
    ) {
        self.apply_event(StagedTurnOrchestratorEvent::StepPatchRecoverReduce(action));
    }

    pub(crate) fn record_post_parse_schedule(&mut self, schedule: PreparedPostParseSchedule) {
        self.apply_event(StagedTurnOrchestratorEvent::PostParseSchedule(schedule));
    }

    pub(crate) fn record_full_pipeline_phase(&mut self, fp: StagedFullPipelinePhase) {
        self.apply_event(StagedTurnOrchestratorEvent::FullPipelinePhase(fp));
    }

    pub(crate) fn record_round_orchestrator(&mut self, phase: StagedRoundOrchestratorPhase) {
        self.apply_event(StagedTurnOrchestratorEvent::RoundOrchestrator(phase));
    }

    /// 定稿 SSE 后进入步队列：同步 `steps_executing_enter` trace 与 round orchestrator 相位。
    pub(crate) fn record_steps_executing_enter(&mut self, phase: StagedRoundOrchestratorPhase) {
        self.record_steps_loop_trace("steps_executing_enter");
        self.record_round_orchestrator(phase);
    }

    pub(crate) fn record_steps_loop_trace(&mut self, steps_loop_phase: &str) {
        self.apply_event(StagedTurnOrchestratorEvent::StepsLoopTrace(
            steps_loop_phase,
        ));
    }

    pub(crate) fn phase_str(&self) -> &'static str {
        self.phase.as_str()
    }

    /// 步队列全部成功跑完后：是否应结束本分阶段回合（早停 / 目标证据），否则 `ContinuePlanning`。
    pub(crate) fn decide_steps_loop_outcome(
        &mut self,
        p: &RunLoopParams<'_>,
        cancelled: bool,
        completed_steps: usize,
        n: usize,
    ) -> StagedPlanRunOutcome {
        if !Self::should_finish_after_success(p, cancelled, completed_steps, n) {
            return StagedPlanRunOutcome::ContinuePlanning;
        }
        self.apply_event(StagedTurnOrchestratorEvent::StepsLoopEarlyFinishSuccess);
        StagedPlanRunOutcome::Finished
    }

    fn should_finish_after_success(
        p: &RunLoopParams<'_>,
        cancelled: bool,
        completed_steps: usize,
        n: usize,
    ) -> bool {
        if cancelled || n == 0 || completed_steps < n {
            return false;
        }
        let messages = p.turn.messages();
        if evaluate_turn_staged_rolling_horizon_early_stop(
            messages,
            p.turn
                .turn_planner_hints
                .staged_last_completed_step_effective_acceptance
                .as_ref(),
            p.ctx.core.effective_working_dir,
        )
        .is_allow()
        {
            return true;
        }
        matches!(
            check_active_user_goal_completion_evidence(messages),
            GoalCompletionEvidenceCheck::Satisfied
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::AgentReplyPlanV1;

    #[test]
    fn driver_records_prepared_terminal() {
        let mut d = StagedTurnDriver::new();
        d.record_parse_terminal(&StagedParseTerminalRoute::DegradeToOuterLoop);
        assert_eq!(d.phase_str(), "degraded_to_outer_loop");
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![],
            no_task: true,
        };
        d.record_parse_terminal(&StagedParseTerminalRoute::ContinueWithPlan { plan });
        assert_eq!(d.phase_str(), "plan_ready");
    }

    #[test]
    fn driver_records_prepared_route_reduce() {
        let mut d = StagedTurnDriver::new();
        d.record_prepared_route_reduce(PreparedRouteReduceAction::DegradeToOuterLoop);
        assert_eq!(d.phase_str(), "degraded_to_outer_loop");
    }

    #[test]
    fn driver_preflight_continue_leaves_phase() {
        let mut d = StagedTurnDriver::new();
        d.phase = StagedTurnOrchestratorPhase::StepRunning;
        d.record_rolling_horizon_preflight(RollingHorizonPreflightAction::ContinueIteration);
        assert_eq!(d.phase_str(), "step_running");
    }
}
