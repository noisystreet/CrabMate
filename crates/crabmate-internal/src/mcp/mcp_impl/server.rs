//! CrabMate 作为 **MCP server**（stdio / TCP）：将内置 `tools::run_tool` 暴露给外部 MCP 客户端。
//!
//! **安全**：无传输层鉴权；与 `run_command` / 工作区策略一致，调用方获得与本地 `crabmate` 相同的执行面。仅应对**可信**父进程 / 本机集成开放。

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer, serve_server};
use serde_json::Value;

use crate::config::AgentConfig;
use crate::tool_call_explain;
use crate::tools;
use crate::types::Tool as OpenAiTool;

/// MCP 形态的 [`Tool`](rmcp::model::Tool)（`input_schema` 与 OpenAI `parameters` 对象对齐）。
pub fn openai_tool_to_mcp_tool(t: &OpenAiTool) -> Option<rmcp::model::Tool> {
    let map = match t.function.parameters.clone() {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    let desc: Cow<'static, str> = if t.function.description.is_empty() {
        Cow::Borrowed("")
    } else {
        Cow::Owned(t.function.description.clone())
    };
    Some(rmcp::model::Tool::new(
        t.function.name.clone(),
        desc,
        Arc::new(map),
    ))
}

/// 构建 `tools/list` 条目（可选 `--no-tools` 时空列表）。
fn build_mcp_tool_list(cfg: &AgentConfig, no_tools: bool) -> Vec<rmcp::model::Tool> {
    if no_tools {
        return Vec::new();
    }
    let mut defs = tools::build_tools();
    tool_call_explain::annotate_tool_defs_for_explain_card(&mut defs, cfg);
    defs.iter().filter_map(openai_tool_to_mcp_tool).collect()
}

/// 持有配置与工作目录；在独立任务中执行 `tools/call` → [`tools::run_tool`]。
#[derive(Clone)]
pub struct CrabmateMcpServer {
    cfg: AgentConfig,
    working_dir: PathBuf,
    mcp_tools: Arc<Vec<rmcp::model::Tool>>,
}

impl CrabmateMcpServer {
    pub fn new(cfg: AgentConfig, working_dir: PathBuf, no_tools: bool) -> Self {
        let mcp_tools = Arc::new(build_mcp_tool_list(&cfg, no_tools));
        Self {
            cfg,
            working_dir,
            mcp_tools,
        }
    }
}

impl ServerHandler for CrabmateMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("crabmate", env!("CARGO_PKG_VERSION"))
                    .with_title("CrabMate built-in tools"),
            )
            .with_instructions(
                "暴露 CrabMate 内置工具（与 `crabmate` 配置中的 run_command 白名单、http_fetch 前缀、工作区路径策略一致）。\
                 无传输层鉴权：仅用于本机可信集成。",
            )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>>
    + rmcp::service::MaybeSendFuture
    + '_ {
        let tools = (*self.mcp_tools).clone();
        std::future::ready(Ok(ListToolsResult {
            tools,
            ..Default::default()
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>>
    + rmcp::service::MaybeSendFuture
    + '_ {
        let name = request.name.to_string();
        let args_map = request.arguments.unwrap_or_default();
        let cfg = self.cfg.clone();
        let work_dir = self.working_dir.clone();
        let allowed = self.cfg.command_exec.allowed_commands.clone();

        async move {
            let args_json = match serde_json::to_string(&Value::Object(args_map)) {
                Ok(s) => s,
                Err(e) => {
                    return Err(McpError::invalid_params(
                        format!("无法序列化工具参数: {e}"),
                        None,
                    ));
                }
            };

            let ctx = tools::tool_context_for(&cfg, allowed.as_ref(), work_dir.as_path());
            let args_for_tool =
                match tool_call_explain::require_explain_for_mutation(&cfg, &name, &args_json) {
                    Ok(cow) => cow.into_owned(),
                    Err(msg) => {
                        return Ok(CallToolResult::error(vec![Content::text(msg)]));
                    }
                };
            let output = tools::run_tool(&name, &args_for_tool, &ctx);
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }
    }
}

/// 在 **stdin/stdout** 上运行 MCP server，直到传输关闭或出错。
pub async fn run_stdio_mcp_server(
    cfg: AgentConfig,
    workspace: PathBuf,
    no_tools: bool,
) -> Result<(), String> {
    eprintln!(
        "crabmate mcp serve：stdio MCP server 已启动（工作目录 {}，工具数 {}）。\
         无鉴权：仅连接可信客户端。",
        workspace.display(),
        if no_tools {
            0
        } else {
            build_mcp_tool_list(&cfg, false).len()
        }
    );

    let service = CrabmateMcpServer::new(cfg, workspace, no_tools);
    let (stdin, stdout) = rmcp::transport::stdio();
    let running = serve_server(service, (stdin, stdout))
        .await
        .map_err(|e| format!("MCP server 初始化失败: {e}"))?;

    match running.waiting().await {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("MCP server 运行结束异常: {e}")),
    }
}

/// 在 **TCP 端口** 上运行 MCP server，接受单个连接后退出。
pub async fn run_tcp_mcp_server(
    cfg: AgentConfig,
    workspace: PathBuf,
    no_tools: bool,
    port: u16,
) -> Result<(), String> {
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("绑定 TCP {} 失败: {e}", addr))?;

    eprintln!(
        "crabmate mcp serve：TCP MCP server 已启动（127.0.0.1:{port}，工作目录 {}，工具数 {}）。\
         无鉴权：仅连接可信客户端。",
        workspace.display(),
        if no_tools {
            0
        } else {
            build_mcp_tool_list(&cfg, false).len()
        }
    );

    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .map_err(|e| format!("接受连接失败: {e}"))?;
        eprintln!("MCP 客户端已连接: {peer}");

        let service = CrabmateMcpServer::new(cfg.clone(), workspace.clone(), no_tools);

        // TcpStream 实现了 AsyncRead + AsyncWrite，可直接用作 transport
        let running = serve_server(service, stream)
            .await
            .map_err(|e| format!("MCP server 初始化失败: {e}"))?;

        match running.waiting().await {
            Ok(_) => eprintln!("MCP 客户端已断开: {peer}"),
            Err(e) => {
                eprintln!("MCP 连接异常: {e}");
            }
        }
        // 继续监听下一个连接
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{
        ClientCapabilities, ClientInfo as ClientInfoMode, Implementation as ClientImpl,
        ServerNotification,
    };
    use rmcp::service::{RoleClient, Service as McpService, serve_client};

    /// Minimal MCP 客户端服务（仅用于测试 TCP 传输）。
    struct TestClientService {
        info: ClientInfoMode,
    }

    impl Default for TestClientService {
        fn default() -> Self {
            Self {
                info: ClientInfoMode::new(
                    ClientCapabilities::default(),
                    ClientImpl::new("crabmate-test", "0.1"),
                ),
            }
        }
    }

    impl McpService<RoleClient> for TestClientService {
        fn get_info(&self) -> ClientInfoMode {
            self.info.clone()
        }

        fn handle_request(
            &self,
            _request: rmcp::model::ServerRequest,
            _context: rmcp::service::RequestContext<RoleClient>,
        ) -> impl std::future::Future<Output = Result<rmcp::model::ClientResult, McpError>> + Send
        {
            async { Ok(rmcp::model::ClientResult::empty(())) }
        }

        fn handle_notification(
            &self,
            _notification: ServerNotification,
            _context: rmcp::service::NotificationContext<RoleClient>,
        ) -> impl std::future::Future<Output = Result<(), McpError>> + Send {
            async { Ok(()) }
        }
    }

    #[tokio::test]
    async fn tcp_server_accepts_and_lists_tools() {
        let cfg = crate::config::load_config(None).expect("default config load");
        let workspace = std::env::temp_dir();

        // 随机端口绑定
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();

        // 后台启动 TCP server（保持 _running 存活直到测试结束）
        let server_handle: tokio::task::JoinHandle<Result<(), String>> = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.map_err(|e| format!("{e}"))?;
            eprintln!("test server accepted: {peer}");
            let service = CrabmateMcpServer::new(cfg, workspace, false);
            let _running = serve_server(service, stream)
                .await
                .map_err(|e| format!("{e}"))?;
            // 保持连接存活直到 test 结束（abort）
            std::future::pending::<()>().await;
            Ok(())
        });

        // 客户端通过 TCP 连接
        let client_stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let client_svc = TestClientService::default();
        let running = serve_client(client_svc, client_stream)
            .await
            .expect("MCP client handshake");

        let tools = running.peer().list_all_tools().await.expect("tools/list");
        assert!(!tools.is_empty(), "should list tools via TCP");
        assert!(tools.iter().all(|t| !t.name.is_empty()));

        // 清理
        server_handle.abort();
    }
}
