//! CrabMate Agent **领域层**：规划产物解析、回合启发式、反思控制器等（与 HTTP / 工具执行编排解耦）。
//!
//! 完整回合编排（`agent_turn` 执行面、`workflow` 执行、PER 协调）在根包 **`crabmate::agent`**，以便注入
//! `tool_registry`、SSE 与 `complete_chat_retrying`；外循环 FSM / reduce / driver / pre-gate reason 与完成判定核
//! （`turn_completion_decision` / `completion_suppression` / `task_level_evidence`）作为纯逻辑子目录
//! **`agent::agent_turn::policy`** 与执行面同处根包，只依赖本模块的纯类型。
//!
//! 依赖链：`cm_types` → `cm_config` → `cm_tools` → **`cm_agent`** → 根包编排。

pub mod acceptance;
/// 单轮 `TurnBudgetCounter` 与 `message_pipeline` / 上下文摘要的联动策略。
pub mod context_budget_pressure;
pub mod message_pipeline;
pub mod per_coord;
/// 权限等级 → 六轴投影与回合 min 链（唯一 `match level` 处；设计 `docs/design/permission_levels.md`）。
pub mod permission;
/// 权限决策的结构化审计事件（设计 §11.2；L4 `AuditOnly` 的放行留痕）。
pub mod permission_audit;
pub mod plan_artifact;
/// 终答规划重写与历史扫描等纯逻辑（侧向 LLM 调用仍在根包 `per_plan_semantic_check`）。
pub mod plan_rewrite;
/// PER 侧向语义校验结果类型（侧向 LLM 调用本身仍在根包）。
pub mod plan_semantic;
pub mod step_executor_policy;
/// 面向用户可见正文的轻量清洗（规划摘要等）。
pub mod text_sanitize;
/// 单轮墙钟预算判定与文案。
pub mod turn_budget;
/// 回合上下文时间线收集（注入 / 裁剪；不发 SSE）。
pub mod context_timeline;
pub mod workflow_reflection_controller;
pub mod workspace_snapshot;

pub use plan_rewrite::PlanRewriteExhaustedReason;
pub use plan_semantic::PlanSemanticLlmOutcome;

#[cfg(test)]
mod acceptance_regression_golden;
#[cfg(test)]
mod plan_artifact_golden;

pub(crate) use crate::cm_types::text_utils::preview_chars;
