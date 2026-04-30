//! `mcp` feature 关闭时的桩实现：保持类型与函数符号，运行时返回明确错误。
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::config::AgentConfig;
use crate::types::Tool;

/// 与完整实现同名的占位类型（无 rmcp 会话）。
#[derive(Debug)]
pub struct McpClientSession;

/// OpenAI 兼容工具名前缀（`mcp__{slug}__{remote_name}`）。
#[inline]
pub fn is_mcp_proxy_tool(name: &str) -> bool {
    name.starts_with("mcp__")
}

pub async fn connect_stdio_client(_cmdline: &str) -> Result<McpClientSession, String> {
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

pub fn try_mcp_tool_name(_cfg: &AgentConfig, _openai_name: &str) -> Option<String> {
    None
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

pub async fn try_open_session_and_tools(
    _cfg: &AgentConfig,
) -> Option<(Arc<Mutex<McpClientSession>>, Vec<Tool>)> {
    None
}

#[derive(Debug, Clone)]
pub struct McpCachedStatus {
    pub fingerprint_matches_config: bool,
    pub slug: Option<String>,
    pub openai_tool_names: Vec<String>,
}

pub async fn cached_mcp_status(_cfg: &AgentConfig) -> McpCachedStatus {
    McpCachedStatus {
        fingerprint_matches_config: false,
        slug: None,
        openai_tool_names: Vec::new(),
    }
}

pub mod server {
    //! MCP server 桩（`mcp` feature 关闭时）。

    use std::path::PathBuf;

    use crate::config::AgentConfig;

    pub async fn run_stdio_mcp_server(
        _cfg: AgentConfig,
        _workspace: PathBuf,
        _no_tools: bool,
    ) -> Result<(), String> {
        Err("本 crabmate 二进制未启用 `mcp` Cargo feature，不支持 `mcp serve`".to_string())
    }
}
