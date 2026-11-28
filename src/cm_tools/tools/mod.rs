//! 工具定义与执行：文件、命令、网络与开发工具
//!
//! 每个子模块对应一类工具，便于扩展新工具。

#[cfg(feature = "archive-tools")]
mod archive;
mod background_job_tools;
mod call_graph_sketch;
pub(crate) mod cargo;
mod code_metrics;
mod code_nav;
mod code_nav_dispatch;
mod code_nav_paths;
// `run_command` 域（#1521 批次 1）：装配 / 执行 / 结果三段已收进 `command/` 子目录，
// 这里只保留一条 glob 承接 —— `crate::cm_tools::tools::{CommandSpec, run_async, …}` 等
// 既有路径全部不变，`command/mod.rs` 用显式 `pub use` 决定暴露哪些符号。
mod command;
pub(crate) use command::parse_run_command_payload_lenient;
pub use command::*;
mod contract_map;
mod diagnostics;
mod docs_health_sweep;
pub use crate::cm_tool_exec::capture_trimmed;
pub use parse_args::parse_args_from_value;
pub use parse_args::parse_args_json;
pub use parse_args::parse_args_typed;
pub use tool_param_types::DelegateTaskArgs;
mod env_var_check;
mod exec;
pub use exec::{
    resolve_workspace_executable, run_command_invocation_targets_workspace_script_or_executable,
};
mod file;
pub use file::canonical_workspace_root;
pub(crate) use file::resolve_for_read_open;
// `PathAccess` 的构造需要跨到 `tool_host`（`read_dir` 的外部路径审批要判「是否落在
// 允许根内」）。只放行 `pub(crate)`：对外仍不是公共 API，避免绕开工具层直接用。
pub(crate) use file::PathAccess;
// 审计正文的路径截断：`cm_internal` 的 `read_dir` 工作区外审批也要用它，避免各写一份。
pub(crate) use file::audit_path_detail;
mod git;
mod github_cli;
mod grep_try;
#[cfg(feature = "web-fetch-tools")]
pub mod http_fetch;
mod json_format;
mod markdown_links;
pub mod output_util;
mod parse_args;
mod patch;
pub(crate) mod python_tools;
mod regex_test;
mod registry_macro;
mod repo_overview;
mod rust_ide;
mod schedule;
mod self_config_info;
mod skill;
mod spell_tools;
#[cfg(feature = "csv-tools")]
mod structured_data;
#[cfg(feature = "csv-tools")]
mod structured_dispatch;
pub mod structured_preview;
mod symbol;
#[cfg(feature = "csv-tools")]
mod table_text;
mod test_result_cache;
#[cfg(feature = "diff-tools")]
mod text_diff;
mod text_truncate;
#[cfg(feature = "schema-tools")]
mod tool_args_coerce;
mod tool_args_validate;
// 定义已下沉到 `crate::cm_tool_registry`（#1510 P3-3 第一批）：两者零 `crate::` 依赖，
// 却被注册表与各工具实现共用。这里保留模块别名，使既有 `super::tool_param_types::…`
// / `crate::cm_tools::tools::tool_param_types::…` 的 131 处引用零涟漪。
// **新代码请直接引 `crate::cm_tool_registry::{param_types, json_schema}::…`**：别名只为
// 兼容既有调用点，不再新增使用者（否则又长成两条路径）。
pub(crate) use crate::cm_tool_registry::json_schema as tool_json_schema;
pub(crate) use crate::cm_tool_registry::param_types as tool_param_types;
mod tool_specs_registry;
mod tool_summary_args;
mod weather;
mod web_search;
mod workspace_image_chat_hint;
mod write_sse_preview;

pub mod dev_tag;

/// #1157：内建工具注册面的单点声明宏，供 `tool_specs_registry/specs/*.inc.rs` 使用。
pub(crate) use registry_macro::builtin_tool;

// `#1530`：`mod.rs` 的实体已拆到下列子模块（原先它是一个 878 行的实体模块）。
// 全部用 glob 承接，`crate::cm_tools::tools::<X>` 的既有路径与可见性不变；
// `cm_tools` 本身是 `pub(crate) mod`，故这些 `pub use` 不会扩大对外 API 面。
mod tool_context;
pub use tool_context::*;
mod specs_index;
pub use specs_index::*;
mod assembly;
pub use assembly::*;
mod dispatch;
pub use dispatch::*;
mod command_compile;
pub use command_compile::*;
// `AssembledCommand` / `command_program_and_args` 是 `pub(crate)`，**不**靠上面的 glob 隐式带出：
// 使用者分布在 `cargo_subcommand` / `python_tools` / `tool_host::background_job`，若哪天有人把
// glob 改成显式列名（"只导那几个 pub 的"），隐式带出会静默消失、报错点却跑到远处。
pub(crate) use command_compile::{AssembledCommand, command_program_and_args};

// 兄弟子模块经 `super::<Name>` 取这两个名字（`runners.rs` 用 `ToolError`，
// `tool_args_validate` 用 `find_spec`）——与拆解前 `tools/mod.rs` 的私有 `use` 同义。
// 其余 import 已随实体搬进各自子模块，这里刻意不留（避免 unused_imports）。
use specs_index::find_spec;

use crate::cm_tool_result::ToolError;

mod runners;
pub use runners::*;

/// 仅 **`cargo test`**：重置少数进程级可变 `static`（`run_command` 秒级限流、**`test_result_cache`** LRU）。
/// 集成测试可在夹具开头调用 crate 根 `reset_process_tool_globals_for_tests`（它再转到此处）。
#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    command::reset_run_command_rate_limit_for_tests();
    test_result_cache::reset_test_result_cache_for_tests();
}

pub use github_cli::GhCtx;
pub use github_cli::web_api;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_write_effect;

#[cfg(test)]
mod tests_summarize;

#[cfg(test)]
mod testing;
