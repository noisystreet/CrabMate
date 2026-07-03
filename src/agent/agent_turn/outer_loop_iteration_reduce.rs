//! 外循环单次迭代 **reflect / 工具后** → 无 IO 的 reduce（表驱动；IO 仍在 **`outer_loop`**）。

use super::outer_loop_fsm::{OuterLoopIterationExit, ReflectBranchCtl};

/// 反思分支 reduce（工具轮之前）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterLoopReflectReduceAction {
    StopOuterLoop,
    ContinueNextIteration,
    ProceedToTools,
}

impl OuterLoopReflectReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::StopOuterLoop => "stop_outer_loop",
            Self::ContinueNextIteration => "continue_next_iteration",
            Self::ProceedToTools => "proceed_to_tools",
        }
    }
}

pub(crate) fn reduce_outer_loop_reflect_branch(
    ctl: ReflectBranchCtl,
) -> OuterLoopReflectReduceAction {
    match ctl {
        ReflectBranchCtl::BreakOuter => OuterLoopReflectReduceAction::StopOuterLoop,
        ReflectBranchCtl::ContinueOuter => OuterLoopReflectReduceAction::ContinueNextIteration,
        ReflectBranchCtl::ProceedToTools => OuterLoopReflectReduceAction::ProceedToTools,
    }
}

pub(crate) fn outer_loop_iteration_exit_from_reflect_reduce(
    action: OuterLoopReflectReduceAction,
) -> Option<OuterLoopIterationExit> {
    match action {
        OuterLoopReflectReduceAction::StopOuterLoop => Some(OuterLoopIterationExit::StopOuterLoop),
        OuterLoopReflectReduceAction::ContinueNextIteration => {
            Some(OuterLoopIterationExit::ContinueNextIteration)
        }
        OuterLoopReflectReduceAction::ProceedToTools => None,
    }
}

pub(crate) fn reduce_outer_loop_post_tools_exit(
    task_level_early_stop: bool,
) -> OuterLoopIterationExit {
    if task_level_early_stop {
        OuterLoopIterationExit::StopOuterLoop
    } else {
        OuterLoopIterationExit::ContinueNextIteration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_reduce_maps_to_exit_or_none() {
        assert_eq!(
            outer_loop_iteration_exit_from_reflect_reduce(reduce_outer_loop_reflect_branch(
                ReflectBranchCtl::BreakOuter
            )),
            Some(OuterLoopIterationExit::StopOuterLoop)
        );
        assert!(
            outer_loop_iteration_exit_from_reflect_reduce(reduce_outer_loop_reflect_branch(
                ReflectBranchCtl::ProceedToTools
            ))
            .is_none()
        );
    }

    #[test]
    fn post_tools_early_stop() {
        assert_eq!(
            reduce_outer_loop_post_tools_exit(true),
            OuterLoopIterationExit::StopOuterLoop
        );
        assert_eq!(
            reduce_outer_loop_post_tools_exit(false),
            OuterLoopIterationExit::ContinueNextIteration
        );
    }
}
