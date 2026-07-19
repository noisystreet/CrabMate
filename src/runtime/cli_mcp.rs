//! `crabmate mcp` 子命令：客户端缓存列表与 **stdio MCP server**。

#[cfg(not(feature = "mcp"))]
use crate::config::AgentConfig;

#[cfg(feature = "mcp")]
mod full {
    use std::path::Path;
    use std::path::PathBuf;

    use crate::config::AgentConfig;
    use crate::runtime::cli::cli_effective_work_dir;

    use crate::mcp::server::ToolCallbacks;

    fn build_mcp_callbacks() -> ToolCallbacks {
        ToolCallbacks::new(
            |cfg: &AgentConfig, no_tools: bool| {
                if no_tools {
                    return Vec::new();
                }
                let mut defs = crate::tools::build_tools();
                crate::tool_call_explain::annotate_tool_defs_for_explain_card(&mut defs, cfg);
                defs
            },
            |name: &str, args_json: &str, work_dir: &Path, cfg: &AgentConfig| {
                let ctx = crate::tools::tool_context_for(
                    cfg,
                    &cfg.command_exec.allowed_commands,
                    work_dir,
                );
                let args_for_tool = match crate::tool_call_explain::require_explain_for_mutation(
                    cfg, name, args_json,
                ) {
                    Ok(cow) => cow.into_owned(),
                    Err(msg) => return Err(msg),
                };
                Ok(crate::tools::run_tool(name, &args_for_tool, &ctx))
            },
        )
    }

    /// 执行 `mcp list`（`probe` 为 true 时按 user-data 尝试建立/刷新进程内 MCP 缓存）。
    ///
    /// `repl_context`：来自 REPL **`/mcp`** 时为 true，无缓存时的提示语指向 **`/mcp probe`** 与「输入用户消息跑一轮」。
    pub async fn run_mcp_list(cfg: &AgentConfig, probe: bool, repl_context: bool) {
        let resolved = crate::mcp::resolve_mcp_config(cfg);
        if probe {
            let _ = crate::mcp::try_open_turn_handle(&resolved).await;
        }
        if !resolved.global_enabled {
            println!("MCP：user-data 中 global_enabled=false，本进程无 MCP 工具。");
            return;
        }
        let enabled: Vec<_> = resolved.enabled_servers().collect();
        if enabled.is_empty() {
            println!(
                "MCP：未配置已启用的 stdio 服务器（见 ~/.local/share/crabmate/mcp_servers.json 或 Web 设置 → MCP）。"
            );
            return;
        }
        let runtime = crate::mcp::mcp_servers_runtime_status(&resolved).await;
        let connected: Vec<_> = runtime.iter().filter(|s| s.connected).collect();
        if connected.is_empty() {
            if probe {
                println!(
                    "MCP：已尝试连接，但无可用缓存会话（见日志 target=crabmate）。\
                     常见原因：子进程启动失败、握手失败或 tools/list 为空。"
                );
            } else if repl_context {
                println!(
                    "MCP：本进程内尚无已缓存 stdio 会话。\
                     可先输入任意用户消息跑一轮，或执行 **/mcp probe** 立即尝试连接。"
                );
            } else {
                println!(
                    "MCP：本进程内尚无已缓存 stdio 会话。\
                     请先在本进程中执行至少一轮对话（`repl` / `chat` / Web `/chat`），\
                     或使用 `crabmate mcp list --probe` 尝试立即连接。"
                );
            }
            for st in &runtime {
                if st.enabled {
                    println!("  [{}] {} (slug={}) — 未连接", st.id, st.name, st.slug);
                }
            }
            return;
        }
        println!(
            "MCP：本进程内已缓存 {}/{} 个已启用服务器",
            connected.len(),
            enabled.len()
        );
        for st in connected {
            println!(
                "  [{}] {} slug={} tools={}",
                st.id,
                st.name,
                st.slug,
                st.openai_tool_names.len()
            );
            for name in &st.openai_tool_names {
                println!("    {name}");
            }
        }
    }

    /// `crabmate mcp serve`：在 stdin/stdout（默认）或 TCP 端口上运行 MCP server（**不要**求 `API_KEY`）。
    pub async fn run_mcp_serve(
        cfg: &AgentConfig,
        workspace_cli: &Option<String>,
        no_tools: bool,
        port: u16,
    ) -> Result<(), String> {
        let workspace: PathBuf =
            cli_effective_work_dir(workspace_cli, &cfg.command_exec.run_command_working_dir);
        if port > 0 {
            let cb = build_mcp_callbacks();
            crate::mcp::server::run_tcp_mcp_server(cfg.clone(), workspace, no_tools, port, cb).await
        } else {
            let cb = build_mcp_callbacks();
            crate::mcp::server::run_stdio_mcp_server(cfg.clone(), workspace, no_tools, cb).await
        }
    }
}

#[cfg(feature = "mcp")]
pub use full::{run_mcp_list, run_mcp_serve};

#[cfg(not(feature = "mcp"))]
pub async fn run_mcp_list(_cfg: &AgentConfig, _probe: bool, _repl_context: bool) {
    println!(
        "本 crabmate 二进制未启用 `mcp` Cargo feature，不支持 MCP 列表/探测。请使用 `cargo build --features mcp` 重新编译。"
    );
}

#[cfg(not(feature = "mcp"))]
#[allow(dead_code)]
pub async fn run_mcp_serve(
    _cfg: &AgentConfig,
    _workspace_cli: &Option<String>,
    _no_tools: bool,
    _port: u16,
) -> Result<(), String> {
    Err("本 crabmate 二进制未启用 `mcp` Cargo feature，不支持 `mcp serve`".to_string())
}
