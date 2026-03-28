//! Agent 回合、上下文裁剪/摘要、PER、工作流与终答规划解析（与 `lib` 中 HTTP 路由、`tools` 实现解耦）。

pub mod agent_turn;
pub mod context_window;
/// 对话 `Message` 变换管道：会话同步步骤编排与供应商出站 `messages` 构造（见模块内说明）。
pub mod message_pipeline;
pub mod per_coord;
pub mod plan_artifact;
mod plan_ensemble;
mod plan_optimizer;
pub mod workflow;
pub mod workflow_reflection_controller;
