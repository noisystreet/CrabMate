//! 工作流编排 MVP：DAG 调度 + 并行执行 + 人工审批节点 + 失败补偿 + SLA 超时
//!
//! 当前实现目标是支持模型通过 `workflow_execute` 一次性下发一个 DAG，并由运行时执行引擎在本地完成编排。
//!
//! 子模块：`model`（规格结构）、`node_tool_role`（可选节点工具角色，与分阶段 **`executor_kind`** 共用 **`step_executor_policy`**）、`dag`（拓扑与依赖校验）、`parse`（JSON→`WorkflowSpec`）、`types`（运行期状态与报告 JSON）、
//! `placeholders`（`{{node.output}}` 等注入）、`execute/`（**`trace`** / **`retry`** / **`node`** / **`schedule`** / **`report`** / **`compensation`**；DAG 调度与单节点执行）、`run`（`workflow_execute` 工具入口）、`chrome_trace`（可选 Chrome Trace JSON 导出）。

mod author_load;
mod author_validate;
mod chrome_trace;
mod compile_spec;
mod dag;
mod execute;
mod for_each_expand;
mod md_extract;
pub mod model;
pub mod node_tool_role;
mod parse;
mod placeholders;
mod run;
mod run_if;
mod types;
mod workflow_templates;

use std::path::Path;

pub use author_validate::{
    AuthorDocumentMode, WORKFLOW_AUTHOR_SPEC_VERSION, validate_workflow_author_document,
};
pub use execute::WorkflowApprovalMode;
pub use node_tool_role::WorkflowNodeToolRole;
pub use run::run_workflow_execute_tool;

/// 作者层 YAML → `{"workflow":{...}}` JSON（`steps` + `after` → `nodes`）。
pub fn compile_workflow_author_yaml(yaml: &str) -> Result<serde_json::Value, String> {
    compile_spec::compile_workflow_author_yaml(yaml)
}

/// 从 Markdown 取首个 `` ```crabmate-workflow `` 块正文。
pub fn extract_first_crabmate_workflow_block(md: &str) -> Result<String, String> {
    md_extract::extract_first_crabmate_workflow_block(md)
}

/// 解析 `workflow_execute` 参数 JSON（含内联 `steps` 编译）。
pub fn parse_workflow_spec_from_json(args_json: &str) -> Result<model::WorkflowSpec, String> {
    parse::parse_workflow_spec(args_json)
}

/// 将 `workflow_file` 展开为内联 `workflow`（Agent / CLI 共用）。
pub fn resolve_workflow_execute_args(
    args_json: &str,
    workspace: &Path,
    workspace_is_set: bool,
) -> Result<String, String> {
    author_load::resolve_workflow_execute_args(args_json, workspace, workspace_is_set)
}

/// 计算 DAG 拓扑层（校验无环）。
pub fn workflow_topo_layers(nodes: &[model::WorkflowNodeSpec]) -> Result<Vec<Vec<String>>, String> {
    dag::topo_layers(nodes)
}

#[cfg(test)]
mod tests;
