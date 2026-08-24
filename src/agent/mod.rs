//! Agent 回合、上下文裁剪/摘要、PER、工作流与终答规划解析（与 `lib` 中 HTTP 路由、`tools` 实现解耦）。
//!
//! **领域层**（规划产物、墙钟预算等）见 **`cm_agent`**，经本模块再导出以保持 `crate::agent::` 路径稳定。

/// 验收规则内核（规范化 spec + 证据 → 判定）。
pub use crate::cm_agent::acceptance;
pub mod agent_turn;
pub use crate::cm_agent::context_budget_pressure;
/// 最终请求 Token 预算、分项估算与完整交互组压缩报告。
pub mod context_compaction;
pub mod context_window;
/// 完整规范历史派生出的单轮模型上下文视图与持久化配方。
pub mod model_context_view;
/// 对话 `Message` 变换管道：会话同步步骤编排与供应商出站 `messages` 构造（见模块内说明）。
pub use crate::cm_agent::message_pipeline;
/// 规划–执行–反思（PER）协调、终答规划门控与重写（领域核在 `cm_agent`）。
pub use crate::cm_agent::per_coord;
mod per_plan_semantic_check;
/// 步级 `executor_kind` 与 DAG `node_tool_role` 共用的工具允许表。
pub(crate) mod step_executor_policy;
/// OpenAI 兼容会话的 **tiktoken** prompt token 粗估（与 `message_pipeline::conversation_messages_to_vendor_body` 对齐）。
pub mod tiktoken_prompt_tokens;
pub use crate::cm_workflow as workflow;
#[cfg(test)]
mod workflow_compile_golden;
#[cfg(test)]
mod workflow_reflection_golden;
/// `workflow_execute` 与 PER 协调的接合点（从 `tool_registry` 拆出以降低 `tool_registry → agent` 耦合）。
pub mod workflow_tool_dispatch;

pub use crate::cm_agent::{
    plan_artifact, text_sanitize, turn_budget, workflow_reflection_controller, workspace_snapshot,
};
