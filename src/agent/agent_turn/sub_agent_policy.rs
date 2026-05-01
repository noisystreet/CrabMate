//! 分阶段规划「子代理」步级工具约束：实现见 [`crate::agent::step_executor_policy`]（与 DAG 节点 `node_tool_role` 共用）。

pub(crate) use crate::agent::step_executor_policy::{
    executor_kind_tool_denied_body, filter_tool_defs_for_executor_kind,
    tool_allowed_for_step_executor_kind,
};
