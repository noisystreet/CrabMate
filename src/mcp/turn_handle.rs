//! 单轮 agent 持有的多 MCP stdio 会话（按 slug 路由 `tools/call`）。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::McpClientSession;
use super::parse_mcp_openai_tool_name;

/// 单轮内按 slug 索引的 MCP 客户端集合。
pub struct McpTurnSessions {
    pub tool_timeout_secs: u64,
    sessions: HashMap<String, Arc<Mutex<McpClientSession>>>,
}

impl McpTurnSessions {
    pub fn new(
        tool_timeout_secs: u64,
        sessions: HashMap<String, Arc<Mutex<McpClientSession>>>,
    ) -> Self {
        Self {
            tool_timeout_secs,
            sessions,
        }
    }

    /// 若 `openai_name` 为 `mcp__{slug}__{remote}`，返回对应会话与远端工具名。
    pub fn session_for_openai_tool(
        &self,
        openai_name: &str,
    ) -> Option<(Arc<Mutex<McpClientSession>>, String)> {
        let (slug, remote) = parse_mcp_openai_tool_name(openai_name)?;
        let sess = self.sessions.get(&slug)?;
        Some((Arc::clone(sess), remote))
    }
}

/// 单轮 MCP 句柄（多 server）。
pub type McpTurnHandle = Arc<McpTurnSessions>;
