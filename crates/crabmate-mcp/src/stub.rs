//! `mcp` feature 关闭时的桩实现：保持类型与函数符号，运行时返回明确错误。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crabmate_config::AgentConfig;
use crabmate_types::McpRemoteToolSummary;
use crabmate_types::Tool;

use crate::resolve::{McpStdioLaunch, ResolvedMcpConfig, ResolvedMcpServer};

/// 与完整实现同名的占位类型（无 rmcp 会话）。
#[derive(Debug)]
pub struct McpClientSession;

/// OpenAI 兼容工具名前缀（`mcp__{slug}__{remote_name}`）。
pub use crabmate_tools::tool_naming::is_mcp_proxy_tool;

pub fn parse_mcp_openai_tool_name(openai_name: &str) -> Option<(String, String)> {
    if !openai_name.starts_with("mcp__") {
        return None;
    }
    let rest = openai_name.strip_prefix("mcp__")?;
    let (slug, remote) = rest.split_once("__")?;
    if slug.is_empty() || remote.is_empty() {
        return None;
    }
    Some((slug.to_string(), remote.to_string()))
}

pub async fn connect_stdio_client(_cmdline: &str) -> Result<McpClientSession, String> {
    Err("本 crabmate 二进制未启用 `mcp` Cargo feature，无法建立 MCP 连接".to_string())
}

pub async fn connect_stdio_client_launch(
    _launch: &McpStdioLaunch,
) -> Result<McpClientSession, String> {
    Err("本 crabmate 二进制未启用 `mcp` Cargo feature，无法建立 MCP 连接".to_string())
}

pub fn mcp_tools_as_openai(
    _server_slug: &str,
    _mcp_tools: &[std::convert::Infallible],
) -> Vec<Tool> {
    Vec::new()
}

pub fn merge_tool_lists(base: Vec<Tool>, extra: Vec<Tool>) -> Vec<Tool> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = base.iter().map(|t| t.function.name.clone()).collect();
    let mut merged = base;
    for t in extra {
        if seen.contains(&t.function.name) {
            continue;
        }
        seen.insert(t.function.name.clone());
        merged.push(t);
    }
    merged
}

pub async fn call_mcp_tool(
    _session: &McpClientSession,
    _remote_name: &str,
    _arguments_json: &str,
    _timeout: Duration,
    _max_out_chars: usize,
) -> String {
    "错误：本构建未启用 `mcp` Cargo feature".to_string()
}

pub async fn clear_mcp_process_cache() {}

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

    pub fn session_for_openai_tool(
        &self,
        _openai_name: &str,
    ) -> Option<(Arc<Mutex<McpClientSession>>, String)> {
        None
    }
}

pub type McpTurnHandle = Arc<McpTurnSessions>;

#[derive(Debug, Clone)]
pub struct McpServerSkipInfo {
    pub id: String,
    pub name: String,
    pub error: String,
}

#[derive(Default)]
pub struct McpTurnOpenResult {
    pub handle: Option<McpTurnHandle>,
    pub tools: Vec<Tool>,
    pub skipped: Vec<McpServerSkipInfo>,
}

impl McpTurnOpenResult {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn into_option_pair(self) -> Option<(McpTurnHandle, Vec<Tool>)> {
        self.handle.map(|h| (h, self.tools))
    }
}

pub async fn try_open_turn_handle(_resolved: &ResolvedMcpConfig) -> McpTurnOpenResult {
    McpTurnOpenResult::empty()
}

#[deprecated(note = "servers 恒空；请使用 crabmate_internal::mcp::try_open_session_and_tools")]
pub async fn try_open_session_and_tools(_cfg: &AgentConfig) -> McpTurnOpenResult {
    McpTurnOpenResult::empty()
}

#[derive(Debug, Clone)]
pub struct McpServerRuntimeStatus {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub connected: bool,
    pub openai_tool_names: Vec<String>,
    pub remote_tools: Vec<McpRemoteToolSummary>,
    pub last_error: Option<String>,
}

pub async fn mcp_servers_runtime_status(
    resolved: &ResolvedMcpConfig,
) -> Vec<McpServerRuntimeStatus> {
    resolved
        .servers
        .iter()
        .map(|srv| McpServerRuntimeStatus {
            id: srv.id.clone(),
            name: srv.name.clone(),
            slug: srv.slug.clone(),
            enabled: srv.enabled,
            connected: false,
            openai_tool_names: Vec::new(),
            remote_tools: Vec::new(),
            last_error: Some("本构建未启用 `mcp` Cargo feature".to_string()),
        })
        .collect()
}

pub async fn probe_mcp_server(server: &ResolvedMcpServer) -> McpServerRuntimeStatus {
    McpServerRuntimeStatus {
        id: server.id.clone(),
        name: server.name.clone(),
        slug: server.slug.clone(),
        enabled: server.enabled,
        connected: false,
        openai_tool_names: Vec::new(),
        remote_tools: Vec::new(),
        last_error: Some("本构建未启用 `mcp` Cargo feature".to_string()),
    }
}

pub mod server {
    //! MCP server 桩（`mcp` feature 关闭时）。

    use std::path::PathBuf;

    use crabmate_config::AgentConfig;

    pub async fn run_stdio_mcp_server(
        _cfg: AgentConfig,
        _workspace: PathBuf,
        _no_tools: bool,
    ) -> Result<(), String> {
        Err("本 crabmate 二进制未启用 `mcp` Cargo feature，不支持 `mcp serve`".to_string())
    }
}
