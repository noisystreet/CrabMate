//! L1 删除 `crabmate-config` 中 `StagedPlanBaselineMode` /
//! `StagedPlanFeedbackMode` 后，本模块提供等价的本地枚举与硬编码默认值。
//!
//! **L3 再删除** `staged/` 目录时一并移除本文件。

/// 首轮定稿计划蓝图模式（与原 `crabmate-config::StagedPlanBaselineMode` 等价）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanBaselineMode {
    ImmutableGoalOnly,
    StrictBaselineSteps,
}

/// 步级反馈模式（与原 `crabmate-config::StagedPlanFeedbackMode` 等价）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanFeedbackMode {
    PatchPlanner,
    #[allow(dead_code)]
    FailFast,
}

// --- 硬编码默认值（原 `StagedPlanningConfig` 各字段的嵌入默认）---

pub(crate) const DEFAULT_STAGED_PLAN_FEEDBACK_MODE: StagedPlanFeedbackMode =
    StagedPlanFeedbackMode::PatchPlanner;
pub(crate) const DEFAULT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM: bool = false;
pub(crate) const DEFAULT_STAGED_PLAN_BASELINE_MODE: StagedPlanBaselineMode =
    StagedPlanBaselineMode::ImmutableGoalOnly;
