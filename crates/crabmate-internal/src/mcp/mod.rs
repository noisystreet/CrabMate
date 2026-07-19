//! [Model Context Protocol](https://modelcontextprotocol.io/)（核心逻辑已迁移至 `crabmate-mcp` crate）。
//!
//! 本模块保留 `resolve_mcp_config`（需读取 user-data）并提供 `crabmate-mcp` 缺失的胶水代码。

mod resolve;

pub use resolve::resolve_mcp_config;

// `crabmate-mcp` 在 `mcp` feature 关闭时只导出 stub 类型/函数；开启后才有完整实现。
// 此处显式列出导出项，避免 `pub use crabmate_mcp::*` 的 resolve 模块与外层冲突。
#[cfg(feature = "mcp")]
pub use crabmate_mcp::{
    McpClientSession, McpServerRuntimeStatus, McpTurnHandle, McpTurnSessions, call_mcp_tool,
    clear_mcp_process_cache, connect_stdio_client, is_mcp_proxy_tool, mcp_servers_runtime_status,
    mcp_tool_openai_name, mcp_tools_as_openai, merge_tool_lists, parse_mcp_openai_tool_name,
    probe_mcp_server, server, try_open_session_and_tools, try_open_turn_handle,
};

#[cfg(not(feature = "mcp"))]
pub use crabmate_mcp::{
    McpClientSession, McpServerRuntimeStatus, McpTurnHandle, McpTurnSessions, call_mcp_tool,
    clear_mcp_process_cache, connect_stdio_client, is_mcp_proxy_tool, mcp_servers_runtime_status,
    mcp_tools_as_openai, merge_tool_lists, parse_mcp_openai_tool_name, probe_mcp_server, server,
    try_open_session_and_tools, try_open_turn_handle,
};
