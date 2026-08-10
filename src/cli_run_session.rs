//! 配置加载之后的 CLI 主路径：`serve` / `bench`（从 [`cli_run::run`] 拆出以降低圈复杂度）。
//! 同进程 `chat|repl|tui` 入口已移除；实现模块暂留待 D2.2 删除。

use std::sync::Arc;

use crate::config::cli::definitions::BenchmarkCliArgs;

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
    pub config_path: Option<String>,
    pub serve_port: Option<u16>,
    pub serve_desktop_ready_json: bool,
    pub http_bind_host: String,
    pub workspace_cli: Option<String>,
    pub with_web: bool,
    pub bench_args: BenchmarkCliArgs,
}

pub(super) async fn run_cli_main_routes(
    args: CliDispatchArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let CliDispatchArgs {
        session,
        config_path,
        serve_port,
        serve_desktop_ready_json,
        http_bind_host,
        workspace_cli,
        with_web,
        bench_args,
    } = args;
    let CliSessionStart {
        cfg_holder,
        client,
        tools,
        process_handles,
        api_key,
    } = session;

    if let Some(port) = serve_port {
        #[cfg(feature = "web")]
        {
            super::run_serve_branch(super::ServeBranchArgs {
                cfg_holder: &cfg_holder,
                config_path: &config_path,
                client,
                tools,
                api_key,
                workspace_cli: &workspace_cli,
                port,
                desktop_ready_json: serve_desktop_ready_json,
                http_bind_host: http_bind_host.as_str(),
                with_web,
                process_handles: Arc::clone(&process_handles),
            })
            .await?;
            return Ok(());
        }
        #[cfg(not(feature = "web"))]
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "本 crabmate 二进制未启用 `web` Cargo feature，不支持 `serve`。请使用默认构建或 `cargo build --features web`。",
            )
            .into());
        }
    }

    if super::run_benchmark_batch_if_requested(
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

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "未指定可执行的子命令。请使用 `crabmate serve`（API）或运维子命令（如 `doctor`）；官方终端为 Client 仓 `crabmate-tui`。同进程 `chat|repl|tui` 入口已移除。",
    )
    .into())
}
