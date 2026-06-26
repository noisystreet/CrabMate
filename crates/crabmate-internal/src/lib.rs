//! CrabMate 内部服务层：工具注册与执行、SSE 协议、工作区、MCP、长期记忆等。
//! 根 crate（`crabmate`）通过 `pub use` 保持 `crate::tools` 等路径稳定。

#![recursion_limit = "512"]

pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_types;
pub use crabmate_types as types;

pub mod agent_errors;
pub mod agent_role_turn;
pub mod agent_turn_prep;
pub mod cargo_metadata;
pub mod clarification_questionnaire;
pub mod context_bootstrap;
pub mod dsml;
pub mod dynamic_tools;
pub mod health;
pub mod health_dep_compat;
pub mod mcp;
pub mod memory;
pub mod observability;
pub mod process_handles;
pub mod read_file_turn_cache;
pub mod readonly_tool_ttl_cache;
pub mod redact;
pub mod request_chrome_trace;
pub mod sse;
pub mod text_encoding;
pub mod text_util;
pub mod tool_approval;
pub mod tool_call_explain;
pub mod tool_registry;
pub mod tool_result;
pub mod tool_sandbox;
pub mod tool_stats;
pub mod tools;
pub mod user_data;
pub mod user_message_file_refs;
pub mod web_static_dir;
pub mod workspace;

pub use process_handles::ProcessHandles;

/// 仅 **`cargo test`**：清空 **`run_command`** 全局限流状态与 **`test_result_cache`** LRU，减轻测试顺序依赖。
#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    tools::reset_process_tool_globals_for_tests();
}
