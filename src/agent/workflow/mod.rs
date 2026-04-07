//! 工作流编排 MVP：DAG 调度 + 并行执行 + 人工审批节点 + 失败补偿 + SLA 超时
//!
//! 当前实现目标是支持模型通过 `workflow_execute` 一次性下发一个 DAG，并由运行时执行引擎在本地完成编排。
//!
//! 子模块：`model`（规格结构）、`dag`（拓扑与依赖校验）、`parse`（JSON→`WorkflowSpec`）、`types`（运行期状态与报告 JSON）、
//! `placeholders`（`{{node.output}}` 等注入）、`execute/`（**`trace`** / **`retry`** / **`node`** / **`schedule`** / **`report`** / **`compensation`**；DAG 调度与单节点执行）、`run`（`workflow_execute` 工具入口）、`chrome_trace`（可选 Chrome Trace JSON 导出）。

mod chrome_trace;
mod dag;
mod execute;
pub mod model;
mod parse;
mod placeholders;
mod run;
mod types;

pub use execute::WorkflowApprovalMode;
pub use run::run_workflow_execute_tool;

#[cfg(test)]
mod tests;
