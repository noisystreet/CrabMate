//! [Model Context Protocol](https://modelcontextprotocol.io/)（核心逻辑已迁移至 `crabmate-mcp` crate）。
//!
//! 本模块保留 `resolve_mcp_config`（需读取 user-data）并提供 `crabmate-mcp` 缺失的胶水代码。

mod resolve;

pub use resolve::resolve_mcp_config;

use crabmate_config::AgentConfig;

// `crabmate-mcp` 在 `mcp` feature 关闭时只导出 stub 类型/函数；开启后才有完整实现。
// 此处显式列出导出项，避免 `pub use crabmate_mcp::*` 的 resolve 模块与外层冲突。
// 注意：不直接再导出 `try_open_session_and_tools`——`crabmate-mcp` 内实现已封死（恒空）；
// agent 回合必须走本模块的 user-data 感知版本。
#[cfg(feature = "mcp")]
pub use crabmate_mcp::{
    McpClientSession, McpServerRuntimeStatus, McpServerSkipInfo, McpTurnHandle, McpTurnOpenResult,
    McpTurnSessions, call_mcp_tool, clear_mcp_process_cache, connect_stdio_client,
    connect_stdio_client_launch, is_mcp_proxy_tool, mcp_servers_runtime_status,
    mcp_tool_openai_name, mcp_tools_as_openai, merge_tool_lists, parse_mcp_openai_tool_name,
    probe_mcp_server, sanitize_mcp_json_schema, server, try_open_turn_handle,
};

#[cfg(not(feature = "mcp"))]
pub use crabmate_mcp::{
    McpClientSession, McpServerRuntimeStatus, McpServerSkipInfo, McpTurnHandle, McpTurnOpenResult,
    McpTurnSessions, call_mcp_tool, clear_mcp_process_cache, connect_stdio_client,
    connect_stdio_client_launch, is_mcp_proxy_tool, mcp_servers_runtime_status,
    mcp_tools_as_openai, merge_tool_lists, parse_mcp_openai_tool_name, probe_mcp_server,
    sanitize_mcp_json_schema, server, try_open_turn_handle,
};

/// 按当前 `AgentConfig` **与 user-data `mcp_servers.json`** 解析并打开 MCP 回合句柄。
///
/// 与已封死的 `crabmate_mcp::try_open_session_and_tools` 不同：此处会加载 Web/CLI 设置页落盘的多服务器配置。
pub async fn try_open_session_and_tools(cfg: &AgentConfig) -> McpTurnOpenResult {
    let resolved = resolve_mcp_config(cfg);
    try_open_turn_handle(&resolved).await
}
