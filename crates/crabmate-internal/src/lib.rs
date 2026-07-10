//! CrabMate 内部服务层：工具注册与执行、SSE 协议、工作区、MCP、长期记忆等。

#![recursion_limit = "512"]

pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_memory;
pub use crabmate_types;
pub use crabmate_types as types;

pub mod agent_errors;
pub mod agent_role_turn;
pub mod agent_turn_prep;
pub mod clarification_sse_bridge;
pub mod context_bootstrap;
pub mod dsml;
pub mod dynamic_tools;
pub mod health;
pub mod long_term_memory_tools;
pub mod mcp;
pub mod memory_tool_hosts;
pub mod observability;
pub mod process_handles;
pub mod readonly_tool_ttl_cache;
pub mod request_chrome_trace;
pub mod sse;
pub mod terminal_session;
pub mod text_util;
pub mod tool_approval;
pub mod tool_call_explain;
pub mod tool_registry;
pub mod tool_sandbox;
pub mod tool_stats;
pub mod user_data;
pub mod user_message_file_refs;
pub mod web_static_dir;

pub use clarification_sse_bridge::clarification_questionnaire_body_if_tool_ok;
pub use crabmate_memory::memory;
pub use crabmate_tools::clarification_questionnaire;
pub use crabmate_tools::{
    cargo_metadata, health_dep_compat, project_metrics, read_file_turn_cache, redact,
    text_encoding, tool_result, tools, workspace,
};

pub use process_handles::ProcessHandles;

#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    tools::reset_process_tool_globals_for_tests();
}
