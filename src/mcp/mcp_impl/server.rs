//! CrabMate 作为 **MCP server**（stdio）：将内置 `tools::run_tool` 暴露给外部 MCP 客户端。
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
pub(crate) fn openai_tool_to_mcp_tool(t: &OpenAiTool) -> Option<rmcp::model::Tool> {
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
        let allowed = self.cfg.allowed_commands.clone();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_to_mcp_non_empty_for_default_build() {
        let cfg = crate::config::load_config(None).expect("default config load");
        let list = build_mcp_tool_list(&cfg, false);
        assert!(
            !list.is_empty(),
            "默认工具集应能映射为至少一条 MCP tools/list 项"
        );
        assert!(list.iter().all(|t| !t.name.is_empty()));
    }
}
