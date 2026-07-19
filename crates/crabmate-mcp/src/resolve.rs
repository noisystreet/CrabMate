//! MCP 配置类型及基础构造。

use crabmate_config::AgentConfig;

/// 单条已启用的 stdio MCP 服务器（运行时视图）。
#[derive(Debug, Clone)]
pub struct ResolvedMcpServer {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub command: String,
    pub enabled: bool,
}

/// 本轮 agent 使用的 MCP 配置。
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

/// 从 `cfg` 构造基础 MCP 配置（无 user-data 覆盖）。
/// `crabmate-internal` 中的 `resolve_mcp_config` 会先加载 user-data 再调用本函数补充。
pub fn resolve_mcp_config(cfg: &AgentConfig) -> ResolvedMcpConfig {
    ResolvedMcpConfig {
        global_enabled: cfg.mcp_client.mcp_enabled,
        tool_timeout_secs: cfg.mcp_client.mcp_tool_timeout_secs.max(1),
        servers: Vec::new(),
    }
}
