//! DAG 节点「工具角色」：与分阶段 `executor_kind`（[`crate::agent::plan_artifact::PlanStepExecutorKind`]）共用同一套允许表（见 [`crate::agent::agent_turn::sub_agent_policy`]），避免在 handler 里再开一套并行语义。
//!
//! **边界**：仅约束**单节点实际调用的 `tool_name`**；不替代 `validate_only` 的 DAG 结构规划，也不替代分阶段「滚动步 + 步级 `tools` 列表收窄」——后者仍在 `agent_turn` / `outer_loop` 中完成。

use serde::{Deserialize, Serialize};

use crate::agent::plan_artifact::PlanStepExecutorKind;

/// 工作流节点在 DAG 内允许的工具类别；JSON 键名 `node_tool_role`（可选 `executor_kind` 作为别名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeToolRole {
    /// 与 `PlanStepExecutorKind::ReviewReadonly` 一致：只读工具，禁 MCP。
    ReviewReadonly,
    /// 与 `PlanStepExecutorKind::PatchWrite` 一致。
    PatchWrite,
    /// 与 `PlanStepExecutorKind::TestRunner` 一致。
    TestRunner,
}

impl WorkflowNodeToolRole {
    /// 映射到分阶段子代理枚举，复用 [`crate::agent::agent_turn::sub_agent_policy::tool_allowed_for_step_executor_kind`]。
    pub fn as_plan_step_executor_kind(self) -> PlanStepExecutorKind {
        match self {
            WorkflowNodeToolRole::ReviewReadonly => PlanStepExecutorKind::ReviewReadonly,
            WorkflowNodeToolRole::PatchWrite => PlanStepExecutorKind::PatchWrite,
            WorkflowNodeToolRole::TestRunner => PlanStepExecutorKind::TestRunner,
        }
    }
}
