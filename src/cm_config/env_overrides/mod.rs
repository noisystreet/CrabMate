//! 从 `CM_*` 环境变量覆盖 [`super::builder::ConfigBuilder`]（优先级高于磁盘 TOML）。
//!
//! 按 `ConfigBuilder` 的配置段分文件：每个子模块只暴露一个
//! `pub(super) fn apply(&mut ConfigBuilder)`，本文件只保留具名顺序分派。
//! 新增一个 `CM_*` 键时按字段所属配置段选文件，不再追加到「最后一个 part」。

use super::builder::ConfigBuilder;

mod agents_md;
mod chat_queue;
mod context;
mod llm;
mod long_term_memory;
mod mcp_codebase;
mod per_plan_policy;
mod permission;
mod roles;
mod run_command;
mod session_changelist;
mod session_ui;
mod skills;
mod tool_runtime;
mod tool_sandbox;
mod tools_network;
mod web_api;
mod workspace;

/// 从 `CM_*` 环境变量覆盖 `ConfigBuilder` 字段。
///
/// 各子模块只写自己那一段字段，彼此不相交，故调用顺序不影响结果；
/// 保留的先后只对齐原 `part_N` 链的首现顺序，便于 diff 比对。
pub(super) fn apply_env_overrides(b: &mut ConfigBuilder) {
    llm::apply(b);
    session_ui::apply(b);
    run_command::apply(b);
    workspace::apply(b);
    tools_network::apply(b);
    per_plan_policy::apply(b);
    roles::apply(b);
    agents_md::apply(b);
    skills::apply(b);
    tool_runtime::apply(b);
    context::apply(b);
    chat_queue::apply(b);
    session_changelist::apply(b);
    tool_sandbox::apply(b);
    web_api::apply(b);
    long_term_memory::apply(b);
    mcp_codebase::apply(b);
    permission::apply(b);
}
