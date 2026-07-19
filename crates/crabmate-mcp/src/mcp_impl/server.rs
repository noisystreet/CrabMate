//! CrabMate 作为 **MCP server**（stdio / TCP）：将内置工具暴露给外部 MCP 客户端。
//!
//! **安全**：无传输层鉴权；与 `run_command` / 工作区策略一致，调用方获得与本地 `crabmate` 相同的执行面。仅应对**可信**父进程 / 本机集成开放。
//!
//! 与 `crabmate-internal` 解耦：工具列表构建与工具执行业务通过 `ToolCallbacks` 注入。

use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer, serve_server};
use serde_json::Value;

use crabmate_config::AgentConfig;
use crabmate_types::Tool as OpenAiTool;

/// 工具调用回调：构建工具列表、检查是否需要审批、执行工具。
#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct ToolCallbacks {
    /// 构建工具列表（`no_tools` 为 true 时返回空列表）。
    pub build_tool_list: Arc<dyn Fn(&AgentConfig, bool) -> Vec<OpenAiTool> + Send + Sync>,
    /// 执行工具（名称、参数字符串、工作目录）→ 输出字符串。
    /// 返回 `Err` 表示执行被阻止（如审批失败）。
    pub run_tool:
        Arc<dyn Fn(&str, &str, &Path, &AgentConfig) -> Result<String, String> + Send + Sync>,
}

impl ToolCallbacks {
    pub fn new(
        build_tool_list: impl Fn(&AgentConfig, bool) -> Vec<OpenAiTool> + Send + Sync + 'static,
        run_tool: impl Fn(&str, &str, &Path, &AgentConfig) -> Result<String, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            build_tool_list: Arc::new(build_tool_list),
            run_tool: Arc::new(run_tool),
        }
    }
}

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

/// 持有配置、工作目录与回调；在独立任务中执行 `tools/call`。
#[derive(Clone)]
pub struct CrabmateMcpServer {
    cfg: AgentConfig,
    working_dir: PathBuf,
    mcp_tools: Arc<Vec<rmcp::model::Tool>>,
    callbacks: ToolCallbacks,
}

impl CrabmateMcpServer {
    pub fn new(
        cfg: AgentConfig,
        working_dir: PathBuf,
        no_tools: bool,
        callbacks: ToolCallbacks,
    ) -> Self {
        let tool_list = (callbacks.build_tool_list)(&cfg, no_tools);
        let mcp_tools = Arc::new(
            tool_list
                .iter()
                .filter_map(openai_tool_to_mcp_tool)
                .collect(),
        );
        Self {
            cfg,
            working_dir,
            mcp_tools,
            callbacks,
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
        let callbacks = self.callbacks.clone();

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

            match (callbacks.run_tool)(&name, &args_json, &work_dir, &cfg) {
                Ok(output) => Ok(CallToolResult::success(vec![Content::text(output)])),
                Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
            }
        }
    }
}

/// 在 **stdin/stdout** 上运行 MCP server，直到传输关闭或出错。
pub async fn run_stdio_mcp_server(
    cfg: AgentConfig,
    workspace: PathBuf,
    no_tools: bool,
    callbacks: ToolCallbacks,
) -> Result<(), String> {
    let tool_count = if no_tools {
        0
    } else {
        (callbacks.build_tool_list)(&cfg, false).len()
    };
    eprintln!(
        "crabmate mcp serve：stdio MCP server 已启动（工作目录 {}，工具数 {}）。\
         无鉴权：仅连接可信客户端。",
        workspace.display(),
        tool_count,
    );

    let service = CrabmateMcpServer::new(cfg, workspace, no_tools, callbacks);
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
    callbacks: ToolCallbacks,
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
            (callbacks.build_tool_list)(&cfg, false).len()
        }
    );

    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .map_err(|e| format!("接受连接失败: {e}"))?;
        eprintln!("MCP 客户端已连接: {peer}");

        let service =
            CrabmateMcpServer::new(cfg.clone(), workspace.clone(), no_tools, callbacks.clone());

        let running = serve_server(service, stream)
            .await
            .map_err(|e| format!("MCP server 初始化失败: {e}"))?;

        match running.waiting().await {
            Ok(_) => eprintln!("MCP 客户端已断开: {peer}"),
            Err(e) => {
                eprintln!("MCP 连接异常: {e}");
            }
        }
    }
}
