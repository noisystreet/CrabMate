//! 分阶段回合 **运行时 driver**（`StagedTurnOrchestratorPhase` 权威 + 步后早停决策）。
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2。

use super::super::params::RunLoopParams;
use super::super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
};
use super::super::turn_completion::evaluate_turn_staged_rolling_horizon_early_stop;
use super::StagedPlanRunOutcome;
use super::full_pipeline_fsm::StagedFullPipelinePhase;
use super::orchestrator::StagedRoundOrchestratorPhase;
use super::prepared_post_parse_fsm::PreparedPostParseSchedule;
use super::staged_parse_terminal::StagedParseTerminalRoute;
use super::turn_fsm::StagedTurnPhase;
use super::turn_orchestrator_fsm::{
    StagedTurnOrchestratorPhase, orchestrator_phase_for_full_pipeline,
    orchestrator_phase_for_post_parse_schedule, orchestrator_phase_for_prepared_route,
    orchestrator_phase_for_round_orchestrator, orchestrator_phase_for_steps_loop_trace,
    orchestrator_phase_for_turn_phase,
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

    pub(crate) fn record_turn_phase(&mut self, turn_phase: StagedTurnPhase) {
        self.phase = orchestrator_phase_for_turn_phase(turn_phase);
    }

    pub(crate) fn record_parse_terminal(&mut self, terminal: &StagedParseTerminalRoute) {
        self.phase = orchestrator_phase_for_prepared_route(&terminal.to_prepared_planner_route());
    }

    pub(crate) fn record_post_parse_schedule(&mut self, schedule: PreparedPostParseSchedule) {
        self.phase = orchestrator_phase_for_post_parse_schedule(schedule);
    }

    pub(crate) fn record_full_pipeline_phase(&mut self, fp: StagedFullPipelinePhase) {
        self.phase = orchestrator_phase_for_full_pipeline(fp);
    }

    pub(crate) fn record_round_orchestrator(&mut self, phase: StagedRoundOrchestratorPhase) {
        self.phase = orchestrator_phase_for_round_orchestrator(phase);
    }

    /// 定稿 SSE 后进入步队列：同步 `steps_executing_enter` trace 与 round orchestrator 相位。
    pub(crate) fn record_steps_executing_enter(&mut self, phase: StagedRoundOrchestratorPhase) {
        self.record_steps_loop_trace("steps_executing_enter");
        self.record_round_orchestrator(phase);
    }

    pub(crate) fn record_steps_loop_trace(&mut self, steps_loop_phase: &str) {
        self.phase = orchestrator_phase_for_steps_loop_trace(steps_loop_phase);
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
        self.phase = StagedTurnOrchestratorPhase::Done;
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
}
