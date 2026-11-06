//! CrabMate 内部服务层：工具注册与执行、工作区、MCP、长期记忆等。
//!
//! SSE 控制面协议与运行时已迁至 [`crate::cm_sse_protocol`] 的 `sse` 模块。

pub(crate) use crate::cm_config;
pub(crate) use crate::cm_config as config;
pub(crate) use crate::cm_memory;
pub(crate) use crate::cm_types;
pub(crate) use crate::cm_types as types;

pub mod agent_errors;
pub mod agent_role_turn;
pub mod agent_turn_prep;
pub mod clarification_sse_bridge;
pub mod context_bootstrap;
pub mod dynamic_tools;
pub mod health;
pub mod long_term_memory_tools;
pub mod mcp;
pub mod memory_tool_hosts;
pub mod observability;
pub mod process_handles;
pub mod readonly_tool_ttl_cache;
pub mod request_chrome_trace;
pub mod session_mode_turn;
pub mod terminal_session;
pub mod text_util;
pub mod tool_approval;
pub mod tool_call_explain;
pub mod tool_jobs;
pub mod tool_registry;
pub mod tool_sandbox;
pub mod tool_stats;
pub mod user_data;
pub mod user_message_file_refs;

pub use clarification_sse_bridge::clarification_questionnaire_body_if_tool_ok;

// **兼容保留**（S4-P1a，见 #1300）：这些名字的本体在 `cm_tools` / `cm_memory`，此处仍转发
// 以维持 `crabmate::cm_internal::*` 的 `#[doc(hidden)]` 可达性。
// 直接摘掉会连带令 `cm_tools` / `cm_memory` 内十余处 `pub use` 失去唯一消费者，触发
// `unused_imports` 与 `dead_code`（CI `-D warnings` 会红）—— 那属于 P2 逐桶搬迁的连带清理。
// **外部不得再经本路径引用**：根上的短名已改为直连本体（`lib_server.rs:33`），
// `cm_internal` 内部 48 处自引用也已改为直连本体。搬迁时随桶一并删除。
pub use crate::cm_memory::memory;
pub use crate::cm_tools::clarification_questionnaire;
pub use crate::cm_tools::{
    cargo_metadata, github_token, health_dep_compat, project_metrics, read_file_turn_cache, redact,
    text_encoding, tool_result, tools, workspace,
};

pub use process_handles::{ProcessHandles, TurnProcessHandles};

#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    crate::cm_tools::tools::reset_process_tool_globals_for_tests();
}
