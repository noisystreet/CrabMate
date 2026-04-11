//! 工具分发注册表：按工具名解析执行策略（workflow / 阻塞+超时 / 同步），Web 与 TUI 共用实现。
//!
//! **`spawn_blocking` 与配置**：进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]（仅增引用计数），闭包内通过 [`tools::tool_context_for`] 借用同一份配置与白名单；`allowed_commands` 在 [`AgentConfig`] 内为 [`std::sync::Arc`] 共享切片，避免每轮工具调用整表克隆。纯 CPU、无阻塞 IO 的少数工具可走 [`policy::sync_default_runs_inline`] 在当前 async 任务上直接执行。
//!
//! 新增「需特殊运行时」的工具：在 [`meta`] 中 `tool_dispatch_registry!` 增一行，并在 [`execute::dispatch_tool`] 的 `match hid` 中补分支。（**`workflow_execute`** 除外：由 **`agent::workflow_tool_dispatch`** 执行，见 **`agent_turn::execute_tools`**。）
//!
//! ## 子模块
//!
//! - [`meta`]：工具名 → 执行类别 / 元数据 / 内部分发 id（`tool_dispatch_registry!`）。
//! - [`policy`]：并行墙钟、只读判定、`SyncDefault` 内联等与 `[tool_registry]` 配置对应。
//! - [`runtime`]：`WebToolRuntime` / `CliToolRuntime` / [`ToolRuntime`]。
//! - [`execute`]：`dispatch_tool` 及各类异步执行路径。

mod execute;
mod meta;
mod policy;
mod runtime;

pub(crate) use execute::prefetch_http_fetch_parallel_approvals;
pub use execute::{DispatchToolParams, dispatch_tool};
pub(crate) use meta::{HandlerId, handler_id_for};
pub use meta::{
    ToolDispatchMeta, ToolExecutionClass, all_dispatch_metadata, execution_class_for_tool,
    try_dispatch_meta,
};
pub use policy::{
    is_readonly_tool, parallel_tool_wall_timeout_secs, tool_calls_allow_parallel_sync_batch,
    tool_ok_for_parallel_readonly_batch_piece,
};
pub use runtime::{CliCommandTurnStats, CliToolRuntime, ToolRuntime, WebToolRuntime};

#[cfg(test)]
mod tests;
