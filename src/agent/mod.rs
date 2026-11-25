//! Agent 回合、上下文裁剪/摘要、PER、工作流与终答规划解析（与 `lib` 中 HTTP 路由、`tools` 实现解耦）。
//!
//! **领域层**（规划产物、墙钟预算等）见 **`cm_agent`**，经本模块再导出以保持 `crate::agent::` 路径稳定。

pub mod agent_turn;
pub use crate::cm_agent::context_budget_pressure;
/// 完成判定第 ② 层的 LLM 实现（#1387 S0.5）。契约在 `cm_agent::completion_judge`。
pub mod completion_judge_llm;
/// 最终请求 Token 预算、分项估算与完整交互组压缩报告。
pub mod context_compaction;
pub mod context_window;
/// 子 Agent 委托实现（#839 P1）：契约在 `cm_tools::delegation_host`。
pub mod delegation;
/// `delegate_task` 的 agent 层调度（#839 P1）：只消费注入的 delegator，不持凭据。
pub mod delegation_dispatch;
/// 完整规范历史派生出的单轮模型上下文视图与持久化配方。
pub mod model_context_view;
/// 子回合装配（#839 P0-4）：`SubTurnSpec` → 隔离的 `TurnRequest`。
pub mod sub_turn;
/// 对话 `Message` 变换管道：会话同步步骤编排与供应商出站 `messages` 构造（见模块内说明）。
pub use crate::cm_agent::message_pipeline;
/// 规划–执行–反思（PER）协调、终答规划门控与重写（领域核在 `cm_agent`）。
pub use crate::cm_agent::per_coord;
mod per_plan_semantic_check;
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
