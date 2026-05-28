//! 从 user-data（及 legacy TOML 一次性导入）解析本轮 MCP 配置。

use crate::config::AgentConfig;
use crate::user_data::load_mcp_servers_with_legacy_import;

/// 单条已启用的 stdio MCP 服务器（运行时视图）。
#[derive(Debug, Clone)]
pub struct ResolvedMcpServer {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub command: String,
    pub enabled: bool,
}

/// 本轮 agent 使用的 MCP 配置（user-data 为真源）。
#[derive(Debug, Clone)]
pub struct ResolvedMcpConfig {
    pub global_enabled: bool,
    pub tool_timeout_secs: u64,
    pub servers: Vec<ResolvedMcpServer>,
}

impl ResolvedMcpConfig {
    pub fn enabled_servers(&self) -> impl Iterator<Item = &ResolvedMcpServer> {
        self.servers
            .iter()
            .filter(|s| s.enabled && !s.command.trim().is_empty())
    }
}

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
