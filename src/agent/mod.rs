//! Agent 回合、上下文裁剪/摘要、PER、工作流与终答规划解析（与 `lib` 中 HTTP 路由、`tools` 实现解耦）。

pub mod agent_turn;
pub mod context_window;
pub mod per_coord;
pub mod plan_artifact;
pub mod workflow;
pub mod workflow_reflection_controller;
