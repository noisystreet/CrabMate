//! [Model Context Protocol](https://modelcontextprotocol.io/)：**客户端**（stdio 子进程）与可选 **服务端**（`mcp serve`，stdio / TCP）。
//!
//! - **客户端**：同一进程内按服务器 id + command **复用**连接，将远端 `tools/list` 合并进 OpenAI 兼容工具表，经 `tools/call` 执行。
//! - **服务端**（[`server`]）：支持 stdio 与 TCP 两种传输模式，将 CrabMate 内置 `tools::run_tool` 暴露给外部 MCP 客户端；**无传输层鉴权**，与 `run_command` / 工作区策略一致。
//!
//! **安全**：`command` 由 user-data 显式指定，等效于允许启动任意子进程；仅应在信任的配置源下启用。输出与错误信息经截断，避免过大响应撑爆上下文。

pub mod server;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use tokio::sync::Mutex as TokioMutex;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientInfo, RawContent,
    ResourceContents,
};
use rmcp::service::{PeerRequestOptions, RequestHandle, RunningService, ServiceError};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::AgentConfig;
use crate::types::{FunctionDef, Tool};
use crate::user_data::McpRemoteToolSummary;

use super::resolve::{ResolvedMcpConfig, ResolvedMcpServer};
use super::turn_handle::{McpTurnHandle, McpTurnSessions};

pub use crabmate_tools::tool_naming::{MCP_PROXY_PREFIX, is_mcp_proxy_tool};

/// 单轮持有的 MCP 客户端（`rmcp` 在 Drop 时会清理子进程）。
pub type McpClientSession = RunningService<RoleClient, ClientInfo>;

pub fn mcp_tool_openai_name(server_slug: &str, tool_name: &str) -> String {
    format!("{MCP_PROXY_PREFIX}{server_slug}__{tool_name}")
}

/// 解析 `mcp__{slug}__{remote}`；`slug` 与 `remote` 均非空时返回。
pub fn parse_mcp_openai_tool_name(openai_name: &str) -> Option<(String, String)> {
    if !openai_name.starts_with(MCP_PROXY_PREFIX) {
        return None;
    }
    let rest = openai_name.strip_prefix(MCP_PROXY_PREFIX)?;
    let (slug, remote) = rest.split_once("__")?;
    if slug.is_empty() || remote.is_empty() {
        return None;
    }
    Some((slug.to_string(), remote.to_string()))
}

/// 连接 MCP server（stdio）。失败时返回 `Err`；调用方可降级为不启用 MCP。
pub async fn connect_stdio_client(cmdline: &str) -> Result<McpClientSession, String> {
    let parts = cmd_mate::split_command_line(cmdline.trim());
    if parts.is_empty() {
        return Err("MCP command 为空或仅空白".to_string());
    }
    let program = parts[0].clone();
    let args: Vec<String> = parts[1..].to_vec();

    let transport = TokioChildProcess::new(Command::new(&program).configure(|c| {
        c.args(&args);
        c.kill_on_drop(true);
    }))
    .map_err(|e| format!("启动 MCP 子进程失败: {e}"))?;

    let info = ClientInfo::new(
        ClientCapabilities::default(),
        rmcp::model::Implementation::new("crabmate", env!("CARGO_PKG_VERSION")),
    );

    let client = info
        .serve(transport)
        .await
        .map_err(|e| format!("MCP 握手失败: {e}"))?;

    Ok(client)
}

fn json_schema_to_parameters(schema: &serde_json::Map<String, Value>) -> Value {
    Value::Object(schema.clone())
}

/// 将 MCP 工具转为 OpenAI `Tool` 列表（名称带 `mcp__` 前缀以免与内建工具冲突）。
pub fn mcp_tools_as_openai(server_slug: &str, mcp_tools: &[rmcp::model::Tool]) -> Vec<Tool> {
    let mut out = Vec::with_capacity(mcp_tools.len());
    for t in mcp_tools {
        let name = mcp_tool_openai_name(server_slug, t.name.as_ref());
        let desc = t
            .description
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_else(|| format!("MCP 工具 `{}`（服务器 `{}`）", t.name, server_slug));
        let params = json_schema_to_parameters(t.input_schema.as_ref());
        out.push(Tool {
            typ: "function".to_string(),
            function: FunctionDef {
                name,
                description: desc,
                parameters: params,
            },
        });
    }
    out
}

fn remote_tool_summaries(mcp_tools: &[rmcp::model::Tool]) -> Vec<McpRemoteToolSummary> {
    mcp_tools
        .iter()
        .map(|t| McpRemoteToolSummary {
            name: t.name.to_string(),
            description: t.description.as_ref().map(|c| c.to_string()),
        })
        .collect()
}

/// 合并内建工具与 MCP 工具（MCP 名冲突时跳过并打日志）。
pub fn merge_tool_lists(base: Vec<Tool>, extra: Vec<Tool>) -> Vec<Tool> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = base.iter().map(|t| t.function.name.clone()).collect();
    let mut merged = base;
    for t in extra {
        if seen.contains(&t.function.name) {
            log::warn!(
                target: "crabmate",
                "MCP 工具名与已有工具冲突，已跳过: {}",
                t.function.name
            );
            continue;
        }
        seen.insert(t.function.name.clone());
        merged.push(t);
    }
    merged
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{prefix}…（已截断，共 {n} 字符）")
    }
}

fn format_call_tool_result(
    r: rmcp::model::CallToolResult,
    max_chars: usize,
) -> Result<String, String> {
    if r.is_error == Some(true) {
        let body = content_to_text(&r.content);
        return Err(if body.is_empty() {
            "MCP 工具返回 is_error".to_string()
        } else {
            truncate_str(&body, max_chars)
        });
    }
    let mut parts = Vec::new();
    let text = content_to_text(&r.content);
    if !text.is_empty() {
        parts.push(text);
    }
    if let Some(sc) = r.structured_content {
        let s = serde_json::to_string_pretty(&sc).unwrap_or_else(|_| sc.to_string());
        if !s.is_empty() && s != "null" {
            parts.push(s);
        }
    }
    let joined = parts.join("\n\n");
    if joined.is_empty() {
        Ok("(MCP 工具无文本内容)".to_string())
    } else {
        Ok(truncate_str(&joined, max_chars))
    }
}

fn content_to_text(contents: &[rmcp::model::Content]) -> String {
    let mut buf = String::new();
    for c in contents {
        let piece = match &c.raw {
            RawContent::Text(t) => t.text.clone(),
            RawContent::Resource(r) => match &r.resource {
                ResourceContents::TextResourceContents { text, .. } => text.clone(),
                _ => "[嵌入资源（非文本）已省略]".to_string(),
            },
            RawContent::Image(_) | RawContent::Audio(_) => "[图像/音频内容已省略]".to_string(),
            RawContent::ResourceLink(_) => "[资源链接已省略]".to_string(),
        };
        if !buf.is_empty() && !piece.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&piece);
    }
    buf
}

/// 执行 MCP `tools/call`（须在 async 上下文中持有 `session` 锁）。
pub async fn call_mcp_tool(
    session: &McpClientSession,
    remote_name: &str,
    arguments_json: &str,
    timeout: Duration,
    max_out_chars: usize,
) -> String {
    let args_map: serde_json::Map<String, Value> = match serde_json::from_str(arguments_json) {
        Ok(Value::Object(m)) => m,
        Ok(Value::Null) => serde_json::Map::new(),
        Ok(_) => {
            return "错误：MCP 工具参数须为 JSON 对象".to_string();
        }
        Err(e) => {
            return format!("错误：无法解析工具参数 JSON: {e}");
        }
    };

    let params =
        CallToolRequestParams::new(Cow::Owned(remote_name.to_string())).with_arguments(args_map);

    let mut peer_opts = PeerRequestOptions::default();
    peer_opts.timeout = Some(timeout);
    let req: RequestHandle<RoleClient> = match session
        .send_cancellable_request(CallToolRequest::new(params).into(), peer_opts)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            return format!("错误：MCP 请求发送失败: {e}");
        }
    };

    let resp = match req.await_response().await {
        Ok(r) => r,
        Err(ServiceError::Timeout { .. }) => {
            return format!("错误：MCP 工具调用超时（{:?}）", timeout);
        }
        Err(e) => {
            return format!("错误：MCP 工具调用失败: {e}");
        }
    };

    match resp {
        rmcp::model::ServerResult::CallToolResult(r) => {
            match format_call_tool_result(r, max_out_chars) {
                Ok(s) => s,
                Err(s) => format!("错误：{s}"),
            }
        }
        _ => "错误：MCP 返回了非 CallToolResult".to_string(),
    }
}

struct McpServerCacheEntry {
    fingerprint: String,
    slug: String,
    session: Arc<Mutex<McpClientSession>>,
    mcp_tools: Vec<Tool>,
    remote_tools: Vec<McpRemoteToolSummary>,
    last_error: Option<String>,
}

impl Clone for McpServerCacheEntry {
    fn clone(&self) -> Self {
        Self {
            fingerprint: self.fingerprint.clone(),
            slug: self.slug.clone(),
            session: Arc::clone(&self.session),
            mcp_tools: self.mcp_tools.clone(),
            remote_tools: self.remote_tools.clone(),
            last_error: self.last_error.clone(),
        }
    }
}

fn server_fingerprint(server: &ResolvedMcpServer) -> String {
    format!("v2\0{}\0{}", server.id, server.command.trim())
}

static MCP_MULTI_CACHE: LazyLock<TokioMutex<HashMap<String, McpServerCacheEntry>>> =
    LazyLock::new(|| TokioMutex::new(HashMap::new()));

/// 丢弃进程内 MCP stdio 缓存（user-data 变更或配置热重载后调用）。
pub async fn clear_mcp_process_cache() {
    let mut guard = MCP_MULTI_CACHE.lock().await;
    guard.clear();
}

async fn open_server_fresh(server: &ResolvedMcpServer) -> Result<McpServerCacheEntry, String> {
    let cmd = server.command.trim();
    if cmd.is_empty() {
        return Err("command 为空".to_string());
    }
    log::info!(
        target: "crabmate",
        "MCP 启动 id={} slug={} command={}",
        server.id,
        server.slug,
        crate::redact::mcp_command_line_for_log(cmd),
    );
    let client = connect_stdio_client(cmd).await?;
    let list = client
        .list_all_tools()
        .await
        .map_err(|e| format!("tools/list 失败: {e}"))?;
    if list.is_empty() {
        return Err("tools/list 为空".to_string());
    }
    let extra = mcp_tools_as_openai(&server.slug, &list);
    log::info!(
        target: "crabmate",
        "MCP 已连接 id={} slug={} tools={}",
        server.id,
        server.slug,
        list.len()
    );
    Ok(McpServerCacheEntry {
        fingerprint: server_fingerprint(server),
        slug: server.slug.clone(),
        session: Arc::new(Mutex::new(client)),
        mcp_tools: extra,
        remote_tools: remote_tool_summaries(&list),
        last_error: None,
    })
}

async fn get_or_open_cached(server: &ResolvedMcpServer) -> Result<McpServerCacheEntry, String> {
    let fp = server_fingerprint(server);
    {
        let guard = MCP_MULTI_CACHE.lock().await;
        if let Some(cached) = guard.get(&server.id)
            && cached.fingerprint == fp
        {
            return Ok(McpServerCacheEntry {
                fingerprint: cached.fingerprint.clone(),
                slug: cached.slug.clone(),
                session: Arc::clone(&cached.session),
                mcp_tools: cached.mcp_tools.clone(),
                remote_tools: cached.remote_tools.clone(),
                last_error: cached.last_error.clone(),
            });
        }
    }
    match open_server_fresh(server).await {
        Ok(entry) => {
            let mut guard = MCP_MULTI_CACHE.lock().await;
            guard.insert(server.id.clone(), entry.clone());
            Ok(entry)
        }
        Err(e) => {
            let mut guard = MCP_MULTI_CACHE.lock().await;
            guard.remove(&server.id);
            Err(e)
        }
    }
}

/// 打开多 server 会话并拉取合并工具列表；失败的服务器跳过。
pub async fn try_open_turn_handle(
    resolved: &ResolvedMcpConfig,
) -> Option<(McpTurnHandle, Vec<Tool>)> {
    if !resolved.global_enabled {
        return None;
    }
    let enabled: Vec<&ResolvedMcpServer> = resolved.enabled_servers().collect();
    if enabled.is_empty() {
        return None;
    }
    let mut sessions = HashMap::new();
    let mut all_tools = Vec::new();
    for srv in enabled {
        match get_or_open_cached(srv).await {
            Ok(entry) => {
                sessions.insert(entry.slug.clone(), Arc::clone(&entry.session));
                all_tools.extend(entry.mcp_tools);
            }
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "MCP 服务器跳过 id={} name={}: {}",
                    srv.id,
                    srv.name,
                    e
                );
            }
        }
    }
    if sessions.is_empty() {
        return None;
    }
    Some((
        Arc::new(McpTurnSessions::new(
            resolved.tool_timeout_secs.max(1),
            sessions,
        )),
        all_tools,
    ))
}

/// 按当前 `AgentConfig` 解析 user-data 并打开 MCP 回合句柄。
pub async fn try_open_session_and_tools(cfg: &AgentConfig) -> Option<(McpTurnHandle, Vec<Tool>)> {
    let resolved = crate::mcp::resolve_mcp_config(cfg);
    try_open_turn_handle(&resolved).await
}

/// 运维用：单 server 缓存状态（不发起新连接）。
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
    let guard = MCP_MULTI_CACHE.lock().await;
    resolved
        .servers
        .iter()
        .map(|srv| {
            let fp = server_fingerprint(srv);
            if let Some(cached) = guard.get(&srv.id)
                && cached.fingerprint == fp
            {
                return McpServerRuntimeStatus {
                    id: srv.id.clone(),
                    name: srv.name.clone(),
                    slug: cached.slug.clone(),
                    enabled: srv.enabled,
                    connected: true,
                    openai_tool_names: cached
                        .mcp_tools
                        .iter()
                        .map(|t| t.function.name.clone())
                        .collect(),
                    remote_tools: cached.remote_tools.clone(),
                    last_error: cached.last_error.clone(),
                };
            }
            McpServerRuntimeStatus {
                id: srv.id.clone(),
                name: srv.name.clone(),
                slug: srv.slug.clone(),
                enabled: srv.enabled,
                connected: false,
                openai_tool_names: Vec::new(),
                remote_tools: Vec::new(),
                last_error: None,
            }
        })
        .collect()
}

/// 探测单条 server（刷新缓存）；返回运行时状态。
pub async fn probe_mcp_server(server: &ResolvedMcpServer) -> McpServerRuntimeStatus {
    let result = get_or_open_cached(server).await;
    match result {
        Ok(entry) => McpServerRuntimeStatus {
            id: server.id.clone(),
            name: server.name.clone(),
            slug: entry.slug,
            enabled: server.enabled,
            connected: true,
            openai_tool_names: entry
                .mcp_tools
                .iter()
                .map(|t| t.function.name.clone())
                .collect(),
            remote_tools: entry.remote_tools,
            last_error: None,
        },
        Err(e) => McpServerRuntimeStatus {
            id: server.id.clone(),
            name: server.name.clone(),
            slug: server.slug.clone(),
            enabled: server.enabled,
            connected: false,
            openai_tool_names: Vec::new(),
            remote_tools: Vec::new(),
            last_error: Some(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_openai_tool_name_roundtrip() {
        let slug = "filesystem";
        let remote = "read_file";
        let openai = mcp_tool_openai_name(slug, remote);
        assert_eq!(
            parse_mcp_openai_tool_name(&openai),
            Some((slug.to_string(), remote.to_string()))
        );
    }

    #[test]
    fn split_sh_c_mcp_json_import_cmdline() {
        let line = "sh -c 'cd /tmp/ws && export RUST_LOG=warn; /bin/mcp-server mcp serve --profile summary'";
        let parts = cmd_mate::split_command_line(line);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "sh");
        assert_eq!(parts[1], "-c");
        assert!(parts[2].contains("mcp-server"));
        assert!(parts[2].contains("cd /tmp/ws"));
    }
}
