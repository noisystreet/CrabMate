//! DAG 节点「工具角色」：与分阶段 `executor_kind` 共用同一套允许表。
//!
//! **边界**：仅约束**单节点实际调用的 `tool_name`**；不替代 `validate_only` 的 DAG 结构规划。

use serde::{Deserialize, Serialize};

/// 分阶段 `executor_kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepExecutorKind {
    /// 仅允许语义只读工具。
    #[default]
    ReviewReadonly,
    /// 只读工具 + 受限写补丁类。
    PatchWrite,
    /// 只读工具 + 常见测试运行器。
    TestRunner,
}

impl PlanStepExecutorKind {
    /// 与规划 JSON / SSE 中 `executor_kind` 字符串一致（蛇形）。
    pub fn as_snake_case_str(self) -> &'static str {
        match self {
            PlanStepExecutorKind::ReviewReadonly => "review_readonly",
            PlanStepExecutorKind::PatchWrite => "patch_write",
            PlanStepExecutorKind::TestRunner => "test_runner",
        }
    }
}

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
    /// 映射到分阶段子代理枚举。
    pub fn as_plan_step_executor_kind(self) -> PlanStepExecutorKind {
        match self {
            WorkflowNodeToolRole::ReviewReadonly => PlanStepExecutorKind::ReviewReadonly,
            WorkflowNodeToolRole::PatchWrite => PlanStepExecutorKind::PatchWrite,
            WorkflowNodeToolRole::TestRunner => PlanStepExecutorKind::TestRunner,
        }
    }
}
