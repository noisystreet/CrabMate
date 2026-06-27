//! CrabMate Agent **领域层**：规划产物解析、意图路由、反思控制器等（与 HTTP / 工具执行编排解耦）。
//!
//! 完整回合编排（`agent_turn` 执行面、`hierarchy`、`workflow` 执行、PER 协调）仍在根包 **`crabmate::agent`**，以便注入 `tool_registry`、SSE 与 `complete_chat_retrying`。
//!
//! 依赖链：`crabmate-types` → `crabmate-config` → **`crabmate-agent`** → `crabmate`（根包编排）。

pub mod acceptance;
pub mod agent_turn;
pub mod evolution;
pub mod intent_l0;
pub mod intent_pipeline;
pub mod intent_router;
mod log_preview;
pub mod message_pipeline;
pub mod plan_artifact;
pub mod step_executor_policy;
/// 面向用户可见正文的轻量清洗（规划摘要等）。
pub mod text_sanitize;
/// 单轮墙钟预算判定与文案。
pub mod turn_budget;
pub mod workflow_reflection_controller;
pub mod workspace_snapshot;

#[cfg(test)]
mod plan_artifact_golden;

pub(crate) use log_preview::preview_chars;
