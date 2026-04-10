//! [Model Context Protocol](https://modelcontextprotocol.io/)：**客户端**（stdio 子进程）与可选 **服务端**（`mcp serve`，stdio）。
//!
//! - **客户端**：同一进程内按配置指纹**复用**一条连接，将远端 `tools/list` 合并进 OpenAI 兼容工具表，经 `tools/call` 执行。
//! - **服务端**（[`server`]）：将 CrabMate 内置 `tools::run_tool` 暴露给外部 MCP 客户端；**无传输层鉴权**，与 `run_command` / 工作区策略一致。
//!
//! **安全**：`mcp_command` 由配置显式指定，等效于允许启动任意子进程；仅应在信任的配置源下启用。输出与错误信息经截断，避免过大响应撑爆上下文。

pub mod server;

use std::borrow::Cow;
use std::sync::Arc;
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

const MCP_TOOL_PREFIX: &str = "mcp__";

/// OpenAI 兼容工具名前缀（`mcp__{slug}__{remote_name}`）。
#[inline]
pub fn is_mcp_proxy_tool(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

/// 单轮持有的 MCP 客户端（`rmcp` 在 Drop 时会清理子进程）。
pub type McpClientSession = RunningService<RoleClient, ClientInfo>;

fn mcp_tool_openai_name(server_slug: &str, tool_name: &str) -> String {
    format!("{MCP_TOOL_PREFIX}{server_slug}__{tool_name}")
}

fn slug_from_command(cmd: &str) -> String {
    let token = cmd.split_whitespace().next().unwrap_or("mcp");
    let base = std::path::Path::new(token)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(token);
    let mut s: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "mcp".to_string() } else { s }
}

/// 解析 `mcp_command`：支持 `program arg1 arg2`（不含引号转义；复杂场景请用包装脚本）。
fn parse_command_line(line: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = line
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() { None } else { Some(parts) }
}

/// 连接 MCP server（stdio）。失败时返回 `Err`；调用方可降级为不启用 MCP。
pub async fn connect_stdio_client(cmdline: &str) -> Result<McpClientSession, String> {
    let parts =
        parse_command_line(cmdline).ok_or_else(|| "mcp_command 为空或仅空白".to_string())?;
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

fn strip_mcp_prefix<'a>(openai_name: &'a str, server_slug: &str) -> Option<Cow<'a, str>> {
    let prefix = format!("{MCP_TOOL_PREFIX}{server_slug}__");
    openai_name
        .strip_prefix(&prefix)
        .map(|s| Cow::Owned(s.to_string()))
}

/// 若 `openai_name` 为本配置下的 MCP 工具名，返回远端 `tools/call` 的 `name`。
pub fn try_mcp_tool_name(cfg: &AgentConfig, openai_name: &str) -> Option<String> {
    if !cfg.mcp_enabled || cfg.mcp_command.trim().is_empty() {
        return None;
    }
    let slug = slug_from_command(cfg.mcp_command.trim());
    strip_mcp_prefix(openai_name, &slug).map(|c| c.into_owned())
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

struct McpProcessCache {
    fingerprint: String,
    session: Arc<Mutex<McpClientSession>>,
    mcp_tools: Vec<Tool>,
}

/// 与配置对应的 MCP 连接指纹；变更 `mcp_command` / 开关后应视为新会话。
fn mcp_connection_fingerprint(cfg: &AgentConfig) -> Option<String> {
    if !cfg.mcp_enabled {
        return None;
    }
    let cmd = cfg.mcp_command.trim();
    if cmd.is_empty() {
        return None;
    }
    Some(format!("v1\0{cmd}"))
}

static MCP_PROCESS_CACHE: TokioMutex<Option<McpProcessCache>> = TokioMutex::const_new(None);

/// 丢弃进程内 MCP stdio 缓存（配置热重载或 `mcp_command` 变更后调用，避免沿用旧子进程）。
pub async fn clear_mcp_process_cache() {
    let mut guard = MCP_PROCESS_CACHE.lock().await;
    *guard = None;
}

/// 新建 stdio 会话并 `tools/list`（不经进程内缓存；供缓存未命中时调用）。
async fn open_mcp_session_fresh(
    cfg: &AgentConfig,
) -> Option<(Arc<Mutex<McpClientSession>>, Vec<Tool>)> {
    if !cfg.mcp_enabled {
        return None;
    }
    let cmd = cfg.mcp_command.trim();
    if cmd.is_empty() {
        log::warn!(target: "crabmate", "mcp_enabled 为 true 但 mcp_command 为空，跳过 MCP");
        return None;
    }
    let slug = slug_from_command(cmd);
    match connect_stdio_client(cmd).await {
        Ok(client) => {
            let list = match client.list_all_tools().await {
                Ok(t) => t,
                Err(e) => {
                    log::warn!(
                        target: "crabmate",
                        "MCP tools/list 失败，本回合不使用 MCP 工具: {}",
                        e
                    );
                    return None;
                }
            };
            if list.is_empty() {
                log::info!(
                    target: "crabmate",
                    "MCP 已连接但 tools/list 为空，关闭连接 slug={}",
                    slug
                );
                return None;
            }
            let extra = mcp_tools_as_openai(&slug, &list);
            log::info!(
                target: "crabmate",
                "MCP 已连接 slug={} tools={}",
                slug,
                list.len()
            );
            Some((Arc::new(Mutex::new(client)), extra))
        }
        Err(e) => {
            log::warn!(target: "crabmate", "MCP 连接失败，本回合不使用 MCP: {}", e);
            None
        }
    }
}

/// 打开会话并拉取工具列表；失败返回 `None`（调用方继续使用仅内建工具）。
///
/// 同一进程内按 **`mcp_enabled` + `mcp_command` 指纹** 复用一条 stdio 连接（REPL / serve 多轮共用），避免每轮重启 MCP 子进程。
pub async fn try_open_session_and_tools(
    cfg: &AgentConfig,
) -> Option<(Arc<Mutex<McpClientSession>>, Vec<Tool>)> {
    let fp = mcp_connection_fingerprint(cfg)?;
    {
        let guard = MCP_PROCESS_CACHE.lock().await;
        if let Some(cached) = guard.as_ref()
            && cached.fingerprint == fp
        {
            return Some((Arc::clone(&cached.session), cached.mcp_tools.clone()));
        }
    }
    let opened = open_mcp_session_fresh(cfg).await;
    let mut guard = MCP_PROCESS_CACHE.lock().await;
    match opened {
        Some((sess, tools)) => {
            *guard = Some(McpProcessCache {
                fingerprint: fp,
                session: Arc::clone(&sess),
                mcp_tools: tools.clone(),
            });
            Some((sess, tools))
        }
        None => {
            // 新连接失败时丢弃缓存，避免配置已改仍保留旧 stdio 会话或误导 `mcp list`。
            *guard = None;
            None
        }
    }
}

/// 运维用：当前进程内 MCP 缓存状态（不发起连接）。
#[derive(Debug, Clone)]
pub struct McpCachedStatus {
    pub fingerprint_matches_config: bool,
    pub slug: Option<String>,
    pub openai_tool_names: Vec<String>,
}

/// 若缓存与当前配置的连接指纹一致，返回已缓存的工具 OpenAI 名列表（`mcp__…`）。
pub async fn cached_mcp_status(cfg: &AgentConfig) -> McpCachedStatus {
    let fp = mcp_connection_fingerprint(cfg);
    let guard = MCP_PROCESS_CACHE.lock().await;
    let Some(fp) = fp.as_ref() else {
        return McpCachedStatus {
            fingerprint_matches_config: false,
            slug: None,
            openai_tool_names: Vec::new(),
        };
    };
    let Some(cached) = guard.as_ref() else {
        return McpCachedStatus {
            fingerprint_matches_config: false,
            slug: None,
            openai_tool_names: Vec::new(),
        };
    };
    if &cached.fingerprint != fp {
        return McpCachedStatus {
            fingerprint_matches_config: false,
            slug: None,
            openai_tool_names: Vec::new(),
        };
    }
    let slug = Some(slug_from_command(cfg.mcp_command.trim()));
    McpCachedStatus {
        fingerprint_matches_config: true,
        slug,
        openai_tool_names: cached
            .mcp_tools
            .iter()
            .map(|t| t.function.name.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_command_strips_path() {
        assert_eq!(slug_from_command("/usr/bin/npx -y foo"), "npx");
        assert_eq!(slug_from_command("uvx mcp-server"), "uvx");
    }

    #[test]
    fn mcp_tool_name_roundtrip() {
        let slug = "npx";
        let remote = "add";
        let openai = mcp_tool_openai_name(slug, remote);
        assert_eq!(strip_mcp_prefix(&openai, slug).unwrap(), remote);
    }
}
