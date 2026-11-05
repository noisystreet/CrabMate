//! 配置加载之后的 CLI 主路径：`serve` / `bench`（从 [`cli_run::run`] 拆出以降低圈复杂度）。
//! 同进程 `chat|repl|tui` 入口与实现已硬删（D2.2）；本模块仅处理 `serve` / `bench` 等仍存活的会话型子命令。

use std::sync::Arc;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::{BenchmarkCliArgs, Commands};

/// `AgentConfig` 已载入并完成 HTTP 客户端与工具表初始化。
pub(super) struct CliSessionStart {
    pub cfg_holder: crate::config::SharedAgentConfig,
    pub client: reqwest::Client,
    pub tools: Vec<crate::types::Tool>,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
    pub api_key: String,
}

pub(super) async fn init_cli_session_start(
    cfg: crate::config::AgentConfig,
    no_tools: bool,
) -> Result<CliSessionStart, Box<dyn std::error::Error>> {
    let api_key = super::read_llm_api_key_from_env_lenient(&cfg);
    let cfg_holder = std::sync::Arc::new(tokio::sync::RwLock::new(cfg));
    {
        let g = cfg_holder.read().await;
        log::info!(
            target: "crabmate",
            "配置已加载 api_base={} model={}",
            g.llm.api_base,
            g.llm.model
        );
    }
    let client = {
        let g = cfg_holder.read().await;
        crate::http_client::build_shared_api_client(&g.llm_http_retry)?
    };
    let mut all_tools = crate::tools::build_tools();
    {
        let g = cfg_holder.read().await;
        crate::tool_call_explain::annotate_tool_defs_for_explain_card(&mut all_tools, &g);
    }
    let tools = if no_tools { Vec::new() } else { all_tools };
    let process_handles = crate::process_handles::ProcessHandles::new_arc(
        std::sync::Arc::new(crate::workspace::changelist::WorkspaceChangelistRegistry::default()),
        std::sync::Arc::new(crate::tool_stats::ToolOutcomeRecorder::new()),
        crate::tool_registry::HandlerLookupTable::default_dispatch(),
        crate::tool_sandbox::default_sync_default_sandbox_backend(),
    );
    Ok(CliSessionStart {
        cfg_holder,
        client,
        tools,
        process_handles,
        api_key,
    })
}

pub(super) struct CliDispatchArgs {
    pub session: CliSessionStart,
    pub global: CliGlobals,
    pub command: Commands,
}

/// `serve` 监听地址：`--host`、`CM_HTTP_HOST`、默认 `127.0.0.1`。
#[cfg(feature = "web")]
fn resolve_http_bind_host(host_opt: Option<&str>) -> String {
    host_opt
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CM_HTTP_HOST")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

pub(super) async fn run_cli_main_routes(
    args: CliDispatchArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let CliDispatchArgs {
        session,
        global,
        command,
    } = args;
    let CliSessionStart {
        cfg_holder,
        client,
        tools,
        process_handles,
        api_key,
    } = session;

    match &command {
        Commands::Serve(serve) => {
            #[cfg(feature = "web")]
            {
                let http_bind_host = resolve_http_bind_host(serve.host.as_deref());
                super::cli_run_serve_branch::run_serve_branch(
                    super::cli_run_serve_branch::ServeBranchArgs {
                        cfg_holder: &cfg_holder,
                        config_path: &global.config_path,
                        client,
                        tools,
                        api_key,
                        workspace_cli: &global.workspace,
                        port: serve.bind_port(),
                        desktop_ready_json: serve.desktop_ready_json,
                        workspace_unset: serve.workspace_unset,
                        http_bind_host: http_bind_host.as_str(),
                        process_handles: Arc::clone(&process_handles),
                    },
                )
                .await
            }
            #[cfg(not(feature = "web"))]
            {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "本 crabmate 二进制未启用 `web` Cargo feature，不支持 `serve`。请使用默认构建或 `cargo build --features web`。",
                )
                .into())
            }
        }
        Commands::Bench(bench) => {
            let bench_args = BenchmarkCliArgs::from(bench);
            if super::cli_run_bench::run_benchmark_batch_if_requested(
                &cfg_holder,
                &client,
                api_key.trim(),
                &tools,
                &bench_args,
            )
            .await?
            {
                return Ok(());
            }
            Err(super::unspecified_subcommand_error())
        }
        // 只有 `serve` / `bench` 会走到这里（`cli_run::dispatch` 已保证）。
        _ => Err(super::unspecified_subcommand_error()),
    }
}

#[cfg(all(test, feature = "web"))]
mod route_tests {
    use super::resolve_http_bind_host;

    #[test]
    fn bind_host_falls_back_to_loopback() {
        // 显式空/空白一律视为未给，回落到默认回环地址（除非进程设了 CM_HTTP_HOST）。
        if std::env::var("CM_HTTP_HOST").is_err() {
            assert_eq!(resolve_http_bind_host(None), "127.0.0.1");
            assert_eq!(resolve_http_bind_host(Some("  ")), "127.0.0.1");
        }
        assert_eq!(resolve_http_bind_host(Some("0.0.0.0")), "0.0.0.0");
    }
}
