//! 分阶段 **`steps` 队列循环** 的纯 reduce（表驱动；IO 仍在 **`steps_loop`**）。

use super::StagedPlanRunOutcome;
use super::step_iteration_fsm::StagedStepIterationCtl;

/// 步队列迭代前守卫（墙钟 / SSE / 取消）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepsLoopPreflightReduceAction {
    Continue,
    BreakCancelled,
}

pub(super) fn reduce_steps_loop_preflight(
    sse_closed: bool,
    user_cancelled: bool,
) -> StepsLoopPreflightReduceAction {
    if sse_closed || user_cancelled {
        StepsLoopPreflightReduceAction::BreakCancelled
    } else {
        StepsLoopPreflightReduceAction::Continue
    }
}

/// 单次步迭代 **`StagedStepIterationCtl`** → 队列状态更新（无 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepsLoopIterationReduceAction {
    RetryCurrentStep { n: usize },
    AdvanceToNextStep { n: usize, completed_steps: usize },
    BreakCancelled,
}

pub(super) fn reduce_steps_loop_iteration_ctl(
    ctl: StagedStepIterationCtl,
) -> StepsLoopIterationReduceAction {
    match ctl {
        StagedStepIterationCtl::RetryCurrentStep { n } => {
            StepsLoopIterationReduceAction::RetryCurrentStep { n }
        }
        StagedStepIterationCtl::AdvanceToNextStep { n, completed_steps } => {
            StepsLoopIterationReduceAction::AdvanceToNextStep { n, completed_steps }
        }
        StagedStepIterationCtl::CancelledAfterOuterOk => {
            StepsLoopIterationReduceAction::BreakCancelled
        }
    }
}

pub(super) fn steps_loop_finish_status(staged_loop_cancelled: bool) -> &'static str {
    if staged_loop_cancelled {
        "cancelled"
    } else {
        "ok"
    }
}

pub(super) fn should_push_steps_loop_separator(
    n: usize,
    staged_loop_cancelled: bool,
    completed_steps: usize,
) -> bool {
    n == 0 || (staged_loop_cancelled && completed_steps == 0)
}

/// driver 缺席时的默认 outcome（与历史行为一致：继续滚动视界规划）。
pub(super) fn steps_loop_outcome_without_driver() -> StagedPlanRunOutcome {
    StagedPlanRunOutcome::ContinuePlanning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_breaks_on_cancel() {
        assert_eq!(
            reduce_steps_loop_preflight(false, true),
            StepsLoopPreflightReduceAction::BreakCancelled
        );
    }

    #[test]
    fn iteration_ctl_maps_advance() {
        assert_eq!(
            reduce_steps_loop_iteration_ctl(StagedStepIterationCtl::AdvanceToNextStep {
                n: 2,
                completed_steps: 1,
            }),
            StepsLoopIterationReduceAction::AdvanceToNextStep {
                n: 2,
                completed_steps: 1,
            }
        );
    }

    #[test]
    fn separator_when_empty_or_cancelled_before_progress() {
        assert!(should_push_steps_loop_separator(0, false, 0));
        assert!(should_push_steps_loop_separator(3, true, 0));
        assert!(!should_push_steps_loop_separator(3, true, 2));
    }
}
