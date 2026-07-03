//! 步失败补丁恢复 **reduce**（表驱动；IO 仍在 **`staged_step_patch_recover`** / **`steps_loop`**）。

use crate::config::StagedPlanFeedbackMode;

use super::staged_step_fsm::{
    staged_patch_budget_after_step_failure, staged_patch_budget_tool_messages_not_ok,
    staged_step_patch_planner_enabled,
};
use super::step_patch_route_fsm::{
    StagedStepPatchFailureKind, resolve_staged_step_patch_failure_kind,
};

/// 补丁恢复入口（与 **`StepIterationReduceAction`** 两条路径对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepPatchRecoverBranch {
    OuterExecOrVerify,
    ToolFailure,
}

/// 补丁恢复计划（纯数据；供 **`StagedStepPatchRecoverSpec`** 构造）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepPatchRecoverPlan {
    pub failure_kind: StagedStepPatchFailureKind,
    pub patch_budget: usize,
    pub steps_loop_phase: &'static str,
}

/// reduce 输出：跳过补丁轮或进入有界 **`staged_step_try_patch_recover`**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepPatchRecoverReduceAction {
    Skip,
    Run(StepPatchRecoverPlan),
}

impl StepPatchRecoverReduceAction {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Run(_) => "run_patch",
        }
    }
}

pub(crate) struct StepPatchRecoverReduceInput {
    pub branch: StepPatchRecoverBranch,
    pub feedback_mode: StagedPlanFeedbackMode,
    pub step_max_retries: Option<u32>,
    pub staged_plan_patch_max_attempts: usize,
    pub step_verify_failed_reason: Option<String>,
    pub has_outer_loop_error: bool,
}

pub(crate) fn reduce_step_patch_recover(
    input: StepPatchRecoverReduceInput,
) -> StepPatchRecoverReduceAction {
    match input.branch {
        StepPatchRecoverBranch::ToolFailure => {
            let patch_budget =
                staged_patch_budget_tool_messages_not_ok(input.staged_plan_patch_max_attempts);
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind: StagedStepPatchFailureKind::ToolMessagesNotOk,
                patch_budget,
                steps_loop_phase: "patch_replanner_tool_failure",
            })
        }
        StepPatchRecoverBranch::OuterExecOrVerify => {
            if !staged_step_patch_planner_enabled(input.feedback_mode) {
                return StepPatchRecoverReduceAction::Skip;
            }
            let patch_budget = staged_patch_budget_after_step_failure(
                input.step_max_retries,
                input.staged_plan_patch_max_attempts,
            );
            if patch_budget == 0 {
                return StepPatchRecoverReduceAction::Skip;
            }
            let failure_kind = resolve_staged_step_patch_failure_kind(
                &input.step_verify_failed_reason,
                input.has_outer_loop_error,
            )
            .unwrap_or(StagedStepPatchFailureKind::OuterLoopError);
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind,
                patch_budget,
                steps_loop_phase: "patch_replanner_attempt",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_skip_when_fail_fast() {
        let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
            branch: StepPatchRecoverBranch::OuterExecOrVerify,
            feedback_mode: StagedPlanFeedbackMode::FailFast,
            step_max_retries: None,
            staged_plan_patch_max_attempts: 2,
            step_verify_failed_reason: Some("exit_code_mismatch".into()),
            has_outer_loop_error: false,
        });
        assert_eq!(action, StepPatchRecoverReduceAction::Skip);
    }

    #[test]
    fn outer_run_on_verify_fail() {
        let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
            branch: StepPatchRecoverBranch::OuterExecOrVerify,
            feedback_mode: StagedPlanFeedbackMode::PatchPlanner,
            step_max_retries: None,
            staged_plan_patch_max_attempts: 2,
            step_verify_failed_reason: Some("exit_code_mismatch".into()),
            has_outer_loop_error: false,
        });
        assert_eq!(
            action,
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind: StagedStepPatchFailureKind::StepVerifyFail {
                    reason: "exit_code_mismatch".into(),
                    empty_execution: false,
                },
                patch_budget: 2,
                steps_loop_phase: "patch_replanner_attempt",
            })
        );
    }

    #[test]
    fn tool_branch_always_runs_with_budget() {
        let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
            branch: StepPatchRecoverBranch::ToolFailure,
            feedback_mode: StagedPlanFeedbackMode::PatchPlanner,
            step_max_retries: None,
            staged_plan_patch_max_attempts: 3,
            step_verify_failed_reason: None,
            has_outer_loop_error: false,
        });
        assert_eq!(
            action,
            StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
                failure_kind: StagedStepPatchFailureKind::ToolMessagesNotOk,
                patch_budget: 3,
                steps_loop_phase: "patch_replanner_tool_failure",
            })
        );
    }
}
