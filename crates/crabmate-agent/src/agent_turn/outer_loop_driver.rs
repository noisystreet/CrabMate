//! 单 Agent **`run_agent_outer_loop`** 运行时 driver（相位观测 + reduce 决策辅助）。
//!
//! **约定**：外循环 `outer_loop_step` 相位变更**只**经本类型的 `record_*`；IO 在根包
//! `agent_turn::outer_loop`。见 [`super::phase_vocabulary`] 与
//! `docs/design/per_state_machine_consolidation.md`。

use super::outer_loop_fsm::{OuterLoopIterationExit, OuterLoopIterationPhase, ReflectBranchCtl};
use super::outer_loop_iteration_reduce::{
    OuterLoopReflectReduceAction, reduce_outer_loop_post_tools_exit,
    reduce_outer_loop_reflect_branch,
};

/// 外循环运行时相位（与 **`tracing`** `outer_loop_step` 对齐）。
#[derive(Debug, Clone)]
pub struct OuterLoopDriver {
    phase: OuterLoopIterationPhase,
    last_reflect: Option<ReflectBranchCtl>,
    last_exit: Option<OuterLoopIterationExit>,
}

impl OuterLoopDriver {
    pub fn new() -> Self {
        Self {
            phase: OuterLoopIterationPhase::IterationEnter,
            last_reflect: None,
            last_exit: None,
        }
    }

    /// 当前 `outer_loop_step` 相位（只读）。
    #[inline]
    pub fn current_phase(&self) -> OuterLoopIterationPhase {
        self.phase
    }

    /// 记录相位；仅合法的外循环步进应调用本方法（根包 `outer_loop`）。
    pub fn record_phase(&mut self, phase: OuterLoopIterationPhase) {
        debug_assert!(
            self.phase_transition_allowed(phase),
            "illegal outer_loop_step {:?} → {:?}",
            self.phase,
            phase
        );
        self.phase = phase;
    }

    pub fn phase_str(&self) -> &'static str {
        self.phase.as_str()
    }

    pub fn record_reflect_branch(&mut self, ctl: ReflectBranchCtl) -> OuterLoopReflectReduceAction {
        self.last_reflect = Some(ctl);
        reduce_outer_loop_reflect_branch(ctl)
    }

    pub fn last_reflect_branch(&self) -> Option<ReflectBranchCtl> {
        self.last_reflect
    }

    pub fn record_iteration_exit(&mut self, exit: OuterLoopIterationExit) {
        self.last_exit = Some(exit);
    }

    pub fn last_iteration_exit(&self) -> Option<OuterLoopIterationExit> {
        self.last_exit
    }

    pub fn decide_post_tools_exit(&self, task_level_early_stop: bool) -> OuterLoopIterationExit {
        reduce_outer_loop_post_tools_exit(task_level_early_stop)
    }

    /// 粗粒度合法转移（同相位重入允许，便于测试重置到 `IterationEnter`）。
    fn phase_transition_allowed(&self, next: OuterLoopIterationPhase) -> bool {
        use OuterLoopIterationPhase::*;
        if self.phase == next {
            return true;
        }
        matches!(
            (self.phase, next),
            (IterationEnter, PrepareContextDone)
                | (PrepareContextDone, AfterPlannerModel)
                | (AfterPlannerModel, ReflectDecided)
                | (ReflectDecided, ToolsExecute)
                // 下一轮迭代：任意相位后回到 IterationEnter
                | (_, IterationEnter)
        )
    }
}

impl Default for OuterLoopDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_phase_sequence() {
        let mut d = OuterLoopDriver::new();
        assert_eq!(d.current_phase(), OuterLoopIterationPhase::IterationEnter);
        d.record_phase(OuterLoopIterationPhase::PrepareContextDone);
        d.record_phase(OuterLoopIterationPhase::AfterPlannerModel);
        let action = d.record_reflect_branch(ReflectBranchCtl::ProceedToTools);
        assert_eq!(action, OuterLoopReflectReduceAction::ProceedToTools);
        d.record_phase(OuterLoopIterationPhase::ReflectDecided);
        d.record_phase(OuterLoopIterationPhase::ToolsExecute);
        let exit = d.decide_post_tools_exit(false);
        d.record_iteration_exit(exit);
        assert_eq!(
            d.last_iteration_exit(),
            Some(OuterLoopIterationExit::ContinueNextIteration)
        );
        d.record_phase(OuterLoopIterationPhase::IterationEnter);
    }

    #[test]
    fn reflect_break_stops_outer() {
        let mut d = OuterLoopDriver::new();
        d.record_phase(OuterLoopIterationPhase::PrepareContextDone);
        d.record_phase(OuterLoopIterationPhase::AfterPlannerModel);
        let action = d.record_reflect_branch(ReflectBranchCtl::BreakOuter);
        assert_eq!(action, OuterLoopReflectReduceAction::StopOuterLoop);
        d.record_phase(OuterLoopIterationPhase::ReflectDecided);
        d.record_iteration_exit(OuterLoopIterationExit::StopOuterLoop);
        assert_eq!(
            d.last_iteration_exit(),
            Some(OuterLoopIterationExit::StopOuterLoop)
        );
    }
}
