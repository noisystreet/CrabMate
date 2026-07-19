//! 从 user-data（及 legacy TOML 一次性导入）解析本轮 MCP 配置。
//!
//! `crabmate-mcp` crate 中只提供了基于 cfg 的基础构造，本模块补全 user-data 加载层。

use crabmate_config::AgentConfig;
use crabmate_mcp::resolve::{ResolvedMcpConfig, ResolvedMcpServer};

use crate::user_data::load_mcp_servers_with_legacy_import;

/// 读取 user-data MCP 列表；空列表时尝试从 TOML `mcp_*` 一次性导入。
pub fn resolve_mcp_config(cfg: &AgentConfig) -> ResolvedMcpConfig {
    let file = load_mcp_servers_with_legacy_import(
        cfg.mcp_client.mcp_enabled,
        cfg.mcp_client.mcp_command.trim(),
        cfg.mcp_client.mcp_tool_timeout_secs,
    );
    let tool_timeout_secs = if file.tool_timeout_secs > 0 {
        file.tool_timeout_secs
    } else {
        cfg.mcp_client.mcp_tool_timeout_secs.max(1)
    };
    ResolvedMcpConfig {
        global_enabled: file.global_enabled,
        tool_timeout_secs,
        servers: file
            .servers
            .into_iter()
            .map(|s| ResolvedMcpServer {
                id: s.id,
                name: s.name,
                slug: s.slug,
                command: s.command,
                enabled: s.enabled,
            })
            .collect(),
    }
}
