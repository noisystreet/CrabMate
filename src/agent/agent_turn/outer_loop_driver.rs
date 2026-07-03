//! 单 Agent **`run_agent_outer_loop`** 运行时 driver（相位观测 + reduce 决策辅助）。
//! 见 `docs/design/per_state_machine_consolidation.md` P/R/E 外层。

use super::outer_loop_fsm::{OuterLoopIterationExit, OuterLoopIterationPhase, ReflectBranchCtl};
use super::outer_loop_iteration_reduce::{
    OuterLoopReflectReduceAction, reduce_outer_loop_post_tools_exit,
    reduce_outer_loop_reflect_branch,
};

/// 外循环运行时相位（与 **`tracing`** `outer_loop_step` 对齐）。
#[derive(Debug, Clone)]
pub(crate) struct OuterLoopDriver {
    pub(crate) phase: OuterLoopIterationPhase,
    pub(crate) last_reflect: Option<ReflectBranchCtl>,
    pub(crate) last_exit: Option<OuterLoopIterationExit>,
}

impl OuterLoopDriver {
    pub(crate) fn new() -> Self {
        Self {
            phase: OuterLoopIterationPhase::IterationEnter,
            last_reflect: None,
            last_exit: None,
        }
    }

    pub(crate) fn record_phase(&mut self, phase: OuterLoopIterationPhase) {
        self.phase = phase;
    }

    pub(crate) fn phase_str(&self) -> &'static str {
        self.phase.as_str()
    }

    pub(crate) fn record_reflect_branch(
        &mut self,
        ctl: ReflectBranchCtl,
    ) -> OuterLoopReflectReduceAction {
        self.last_reflect = Some(ctl);
        reduce_outer_loop_reflect_branch(ctl)
    }

    pub(crate) fn record_iteration_exit(&mut self, exit: OuterLoopIterationExit) {
        self.last_exit = Some(exit);
    }

    pub(crate) fn decide_post_tools_exit(
        &self,
        task_level_early_stop: bool,
    ) -> OuterLoopIterationExit {
        reduce_outer_loop_post_tools_exit(task_level_early_stop)
    }
}
