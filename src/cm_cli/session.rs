//! 会话型子命令（`serve` / `bench`）的公共起点：配置 → HTTP client → 工具表 → 进程句柄。
//!
//! 只有这两个命令需要 `ProcessHandles` 与完整工具表；其它命令走
//! [`super::dispatch_offline`] / [`super::dispatch_toolish`]，按需自己加载配置。

use std::sync::Arc;

use crate::cm_config::cli::CliGlobals;
use crate::cm_config::cli::definitions::Commands;

/// `AgentConfig` 已载入并完成 HTTP 客户端与工具表初始化。
pub(super) struct CliSessionStart {
    pub(super) cfg_holder: crate::config::SharedAgentConfig,
    pub(super) client: reqwest::Client,
    pub(super) tools: Vec<crate::types::Tool>,
    pub(super) process_handles: Arc<crate::process_handles::ProcessHandles>,
    pub(super) api_key: String,
}

pub(super) async fn init_cli_session_start(
    cfg: crate::config::AgentConfig,
    no_tools: bool,
) -> Result<CliSessionStart, Box<dyn std::error::Error>> {
    let api_key = super::config_load::read_llm_api_key_from_env_lenient(&cfg);
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

/// `serve` / `bench`：载入配置、初始化会话句柄，再分两条主路径。
pub(super) async fn run(
    global: CliGlobals,
    command: Commands,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = super::config_load::load_cli_agent_config(
        global.config_path.as_deref(),
        global.llm_context_tokens,
    )?;
    if let Commands::Serve(serve) = &command
        && serve.yes
    {
        cfg.command_exec.allowed_commands = std::sync::Arc::from(vec!["*".to_string()]);
        log::warn!(
            target: "crabmate",
            "serve --yes：已跳过命令白名单（allowed_commands → \"*\"），run_command 不再校验命令名；仅请在完全信任或已隔离的环境使用"
        );
    }
    let session = init_cli_session_start(cfg, global.no_tools).await?;

    match command {
        #[cfg(feature = "web")]
        Commands::Serve(serve) => super::serve::run(session, &global, &serve).await,
        #[cfg(not(feature = "web"))]
        Commands::Serve(_) => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "本 crabmate 二进制未启用 `web` Cargo feature，不支持 `serve`。请使用默认构建或 `cargo build --features web`。",
        )
        .into()),
        Commands::Bench(bench) => super::bench::run(session, &bench).await,
        // `super::dispatch` 只把 `serve` / `bench` 送到这里。
        _ => Err(super::unspecified_subcommand_error()),
    }
}
