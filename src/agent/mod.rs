//! Agent 回合、上下文裁剪/摘要、PER、工作流与终答规划解析（与 `lib` 中 HTTP 路由、`tools` 实现解耦）。

pub mod agent_turn;
pub mod context_window;
/// Agent 自我进化模块：决策历史记录、策略分析、system prompt 动态注入。
pub mod evolution;
/// 分层多 Agent 协作架构：Router + Manager + Operator
pub mod hierarchy;
pub mod intent_pipeline;
pub mod intent_router;
/// 对话 `Message` 变换管道：会话同步步骤编排与供应商出站 `messages` 构造（见模块内说明）。
pub mod message_pipeline;
pub mod per_coord;
mod per_plan_semantic_check;
pub mod plan_artifact;
mod plan_ensemble;
mod plan_optimizer;
/// 终答后规划重写控制器（从 `per_coord` 迁出）：策略模式、语义校验开关、重写次数管理。
pub mod plan_rewrite_controller;
/// 终答规划重写与历史扫描等纯逻辑（侧向 LLM 调用仍在 `per_plan_semantic_check`）。
pub mod reflection;
pub mod step_verifier;
pub mod workflow;
pub mod workflow_reflection_controller;
/// `workflow_execute` 与 PER 协调的接合点（从 `tool_registry` 拆出以降低 `tool_registry → agent` 耦合）。
pub mod workflow_tool_dispatch;
pub mod workspace_snapshot;
