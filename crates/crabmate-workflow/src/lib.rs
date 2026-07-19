//! 工作流 DAG 编排引擎：一次性下发 DAG，由运行时引擎本地完成编排。
//!
//! 模块结构与原 `src/agent/workflow/` 一致，外部依赖通过 `crabmate-types` 和
//! 参数化 trait 提供。

// 公开模块（与 mod.rs 出口一致）
pub mod config;
pub mod model;
pub mod node_tool_role;

// 内部模块（原 pub(crate)）
mod author_load;
mod author_validate;
mod chrome_trace;
mod compile_spec;
mod dag;
mod execute;
mod for_each_expand;
mod md_extract;
mod parse;
mod placeholders;
mod resolve_json_path;
mod run;
mod run_if;
mod tests;
mod types;
mod workflow_templates;

// 公开 re-export
pub use author_validate::{
    AuthorDocumentMode, WORKFLOW_AUTHOR_SPEC_VERSION, validate_workflow_author_document,
};
pub use config::WorkflowConfig;
pub use execute::WorkflowApprovalMode;
pub use node_tool_role::WorkflowNodeToolRole;
pub use run::run_workflow_execute_tool;

pub use author_load::resolve_workflow_execute_args;
pub use compile_spec::compile_workflow_author_yaml;
pub use dag::topo_layers as workflow_topo_layers;
pub use md_extract::extract_first_crabmate_workflow_block;
pub use parse::parse_workflow_spec as parse_workflow_spec_from_json;
