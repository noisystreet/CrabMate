//! `PlanStepExecutorKind` 工具允许表（实现于 **`crabmate-agent`**）。

pub(crate) use crabmate_agent::step_executor_policy::{
    executor_kind_user_label, filter_tool_defs_for_executor_kind,
    tool_allowed_for_step_executor_kind,
};
