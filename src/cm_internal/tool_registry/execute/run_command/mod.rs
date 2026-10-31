//! `run_command` 的执行流水线：解析 → 策略判定与审批 → 分派（sync / async / Docker）→ 输出分块。
//!
//! 这些阶段原先各自是一个 `include!` 分片（`execute_run_command_*.inc.rs`），文本被拼进
//! `execute` 的**同一个作用域**：`super::` 的指向依赖拼接位置，改动时漏改一片编译器也无从
//! 发现（#1158 / #1196）。
//!
//! 现在每个阶段是独立子模块，跨阶段调用只能经本文件的 `pub(super) use` 显式接口。
//! 行数上限（`file ≤ 920`）**保持不变**——本模块的目的不是把分片合并成大文件，而是把
//! 「同作用域文本拼接」换成「有边界的模块」。

mod approval;
mod async_invocation;
mod chunks;
mod docker;
mod facade;
mod host;
mod sync;

// 显式接口：**逐项列出**本流水线对 `execute` 暴露的东西，兄弟子模块也只能经由这里
// 互相调用。不用 glob 重导出是有意的——glob 会把「谁依赖谁」重新藏起来，正是本 Issue
// 要消除的问题。
pub(super) use approval::{
    ExecMode, ExternalPathGate, ParsedRunCommandJson, PolicyApproval, PolicyCapabilities,
    PolicyContext, PolicyDecision, PolicyError, RunCommandCaps, approve_external_run_command_paths_if_needed,
    approve_posix_shell_wrap_if_needed, evaluate_policy, parse_run_command_json,
    posix_shell_wrap_needs_interactive_approval,
};
pub(super) use async_invocation::{RunCommandAsyncInvoke, execute_run_command_async};
pub(super) use chunks::{
    RunCommandHostInvoke, emit_run_command_tool_output_chunk, run_command_chunk_sink,
};
pub(super) use docker::dispatch_non_sync_tool_to_docker;
pub(super) use facade::{
    RunCommandCapabilities, RunCommandFacadeInvoke, execute_run_command_facade,
};
pub(super) use host::execute_run_command_impl;
pub(super) use sync::{RunCommandSyncHostInvoke, execute_run_command_sync_host};

// 这条对 `crate` 可见：`tool_registry/mod.rs` 以 `pub use execute::{…}` 对外重导出它。
pub use host::prefetch_http_fetch_parallel_approvals;
