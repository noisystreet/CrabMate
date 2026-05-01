//! 分阶段规划 **步执行循环**（`run_staged_plan_steps_loop`）内的纯决策：补丁轮预算与是否进入补丁路径。
//! **不**发起 LLM；副作用留在 `mod.rs`。

use crate::config::StagedPlanFeedbackMode;

/// 单步失败后触发补丁规划员的尝试次数上限（与 `run_staged_plan_steps_loop` 中原 `for _ in 0..max_retries` 一致）。
#[inline]
pub(crate) fn staged_patch_budget_after_step_failure(
    step_max_retries: Option<u32>,
    cfg_staged_plan_patch_max_attempts: usize,
) -> usize {
    step_max_retries.unwrap_or(cfg_staged_plan_patch_max_attempts as u32) as usize
}

/// 「工具未全部成功」分支的补丁尝试上限（仅使用全局配置计数）。
#[inline]
pub(crate) fn staged_patch_budget_tool_messages_not_ok(
    cfg_staged_plan_patch_max_attempts: usize,
) -> usize {
    cfg_staged_plan_patch_max_attempts
}

/// 是否启用补丁规划员路径（与 `staged_plan_feedback_mode == PatchPlanner` 等价）。
#[inline]
pub(crate) fn staged_step_patch_planner_enabled(mode: StagedPlanFeedbackMode) -> bool {
    matches!(mode, StagedPlanFeedbackMode::PatchPlanner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_matches_loop_upper_bound() {
        assert_eq!(staged_patch_budget_after_step_failure(Some(5), 10), 5);
        assert_eq!(staged_patch_budget_after_step_failure(None, 7), 7);
        assert_eq!(staged_patch_budget_after_step_failure(Some(0), 3), 0);
        assert_eq!(staged_patch_budget_tool_messages_not_ok(0), 0);
    }

    #[test]
    fn patch_only_in_patch_planner_mode() {
        assert!(staged_step_patch_planner_enabled(
            StagedPlanFeedbackMode::PatchPlanner
        ));
        assert!(!staged_step_patch_planner_enabled(
            StagedPlanFeedbackMode::FailFast
        ));
    }
}
