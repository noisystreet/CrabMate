//! 外循环单次迭代 **reflect / 工具后** → 无 IO 的 reduce（表驱动；IO 仍在根包 **`outer_loop`**）。

use super::outer_loop_fsm::{OuterLoopIterationExit, ReflectBranchCtl};

/// 反思分支 reduce（工具轮之前）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterLoopReflectReduceAction {
    StopOuterLoop,
    ContinueNextIteration,
    ProceedToTools,
}

impl OuterLoopReflectReduceAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StopOuterLoop => "stop_outer_loop",
            Self::ContinueNextIteration => "continue_next_iteration",
            Self::ProceedToTools => "proceed_to_tools",
        }
    }
}

pub fn reduce_outer_loop_reflect_branch(ctl: ReflectBranchCtl) -> OuterLoopReflectReduceAction {
    match ctl {
        ReflectBranchCtl::BreakOuter => OuterLoopReflectReduceAction::StopOuterLoop,
        ReflectBranchCtl::ContinueOuter => OuterLoopReflectReduceAction::ContinueNextIteration,
        ReflectBranchCtl::ProceedToTools => OuterLoopReflectReduceAction::ProceedToTools,
    }
}

pub fn outer_loop_iteration_exit_from_reflect_reduce(
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

pub fn reduce_outer_loop_post_tools_exit(task_level_early_stop: bool) -> OuterLoopIterationExit {
    if task_level_early_stop {
        OuterLoopIterationExit::StopOuterLoop
    } else {
        OuterLoopIterationExit::ContinueNextIteration
    }
}

/// 工具执行后：若已注入「截断续写纠偏」则**优先继续下一轮**（收敛优先于任务级早停），
/// 否则退化为 [`reduce_outer_loop_post_tools_exit`]。
///
/// 覆盖「截断 + 工具调用」路径——该轮经 `ProceedToExecuteTools` 跳过 stop-turn 纠偏，
/// 工具结果回到下一轮模型前必须继续外循环，才能消费刚注入的纠偏。
pub fn reduce_outer_loop_post_tools_exit_with_truncation(
    truncated_feedback_injected: bool,
    task_level_early_stop: bool,
) -> OuterLoopIterationExit {
    if truncated_feedback_injected {
        OuterLoopIterationExit::ContinueNextIteration
    } else {
        reduce_outer_loop_post_tools_exit(task_level_early_stop)
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

    #[test]
    fn post_tools_truncation_feedback_overrides_early_stop() {
        // 已注入截断纠偏 → 无论是否允许任务级早停，都继续下一轮以消费纠偏。
        assert_eq!(
            reduce_outer_loop_post_tools_exit_with_truncation(true, true),
            OuterLoopIterationExit::ContinueNextIteration
        );
        assert_eq!(
            reduce_outer_loop_post_tools_exit_with_truncation(true, false),
            OuterLoopIterationExit::ContinueNextIteration
        );
        // 未注入 → 退化为原有早停语义。
        assert_eq!(
            reduce_outer_loop_post_tools_exit_with_truncation(false, true),
            OuterLoopIterationExit::StopOuterLoop
        );
        assert_eq!(
            reduce_outer_loop_post_tools_exit_with_truncation(false, false),
            OuterLoopIterationExit::ContinueNextIteration
        );
    }
}
