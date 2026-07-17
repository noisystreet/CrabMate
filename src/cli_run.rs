//! `cargo run` / 库入口 [`crate::run`] 的 CLI 编排（从 `lib.rs` 拆出以降低单函数圈复杂度）。

#[cfg(feature = "web")]
#[path = "cli_run_serve.rs"]
mod cli_run_serve;

#[path = "cli_run_session.rs"]
mod cli_run_session;

use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use log::info;

#[cfg(feature = "web")]
use crate::AppState;
#[cfg(feature = "web")]
use crate::chat_job_queue;
use crate::config;
use crate::config::cli::{
    ExtraCliCommand, ParsedCliArgs, PluginInitCli, PluginListCli, PluginValidateCli,
    SaveSessionCli, WorkflowFileCli, parse_args,
};
use crate::http_client;
use crate::observability;
use crate::runtime;
#[cfg(feature = "web")]
use crate::web;
use crate::web_static_dir;

/// `crabmate models` / `crabmate probe`：`bearer` 时仍要求进程环境变量 **`API_KEY`** 非空。
fn require_api_key_for_cli_models_probe(
    cfg: &config::AgentConfig,
) -> Result<String, std::io::Error> {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
        eprintln!(
            "请设置环境变量 API_KEY（当前 llm_http_auth_mode=bearer；models/probe 须从环境读取密钥）"
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "未设置环境变量 API_KEY",
        ));
    }
    Ok(v)
}

/// `serve` / `repl` / `chat` / `bench`：读取 **`API_KEY`**；`bearer` 且未设置时返回空串（不报错）。
fn read_llm_api_key_from_env_lenient(cfg: &config::AgentConfig) -> String {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
        info!(
            target: "crabmate",
            "API_KEY 未设置（llm_http_auth_mode=bearer）：Web 请在侧栏设置中填写 API 密钥；REPL 请使用 /api-key set <密钥>"
        );
    }
    v
}

fn apply_cli_llm_context_tokens_override(
    mut cfg: config::AgentConfig,
    cli_tokens: Option<u32>,
) -> config::AgentConfig {
    if let Some(n) = cli_tokens {
        cfg.llm_sampling.llm_context_tokens = n.min(10_000_000);
    }
    cfg
}

fn load_cli_config_for_early_command(
    config_path: &Option<String>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<config::AgentConfig, Box<dyn std::error::Error>> {
    Ok(apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path.as_deref())?,
        llm_context_tokens_cli,
    ))
}

struct EarlyCliDispatch<'a> {
    config_path: &'a Option<String>,
    workspace_cli: &'a Option<String>,
    extra_cli: ExtraCliCommand,
    save_session: Option<SaveSessionCli>,
    tool_replay: Option<crate::config::cli::ToolReplayCli>,
    sse_replay: Option<crate::config::cli::SseReplayCli>,
    plugin_init: Option<PluginInitCli>,
    plugin_validate: Option<PluginValidateCli>,
    plugin_list: Option<PluginListCli>,
    workflow_validate: Option<WorkflowFileCli>,
    workflow_compile: Option<WorkflowFileCli>,
    workflow_run: Option<WorkflowFileCli>,
}

fn try_early_save_session(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(ss) = d.save_session.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli::run_save_session_command(&cfg, d.workspace_cli, ss)?;
    Ok(true)
}

fn try_early_tool_replay(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(tr) = d.tool_replay.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli::run_tool_replay_command(&cfg, d.workspace_cli, tr)?;
    Ok(true)
}

fn try_early_sse_replay(d: &EarlyCliDispatch<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(sr) = d.sse_replay.clone() else {
        return Ok(false);
    };
    crate::runtime::cli::run_sse_replay_command(sr)?;
    Ok(true)
}

fn try_early_plugin_init(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pi) = d.plugin_init.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli::run_plugin_init_command(&cfg, d.workspace_cli, pi)?;
    Ok(true)
}

fn try_early_plugin_validate(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pv) = d.plugin_validate.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli::run_plugin_validate_command(&cfg, d.workspace_cli, pv)?;
    Ok(true)
}

fn try_early_plugin_list(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(pl) = d.plugin_list.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli::run_plugin_list_command(&cfg, d.workspace_cli, pl)?;
    Ok(true)
}

fn try_early_workflow_validate(
    d: &EarlyCliDispatch<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wv) = d.workflow_validate.clone() else {
        return Ok(false);
    };
    crate::runtime::cli_workflow::run_workflow_validate_command(&wv)
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

fn try_early_workflow_compile(
    d: &EarlyCliDispatch<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wc) = d.workflow_compile.clone() else {
        return Ok(false);
    };
    crate::runtime::cli_workflow::run_workflow_compile_command(&wc)
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

async fn try_early_workflow_run(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(wr) = d.workflow_run.clone() else {
        return Ok(false);
    };
    let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
    crate::runtime::cli_workflow::run_workflow_run_command(&cfg, d.workspace_cli, wr)
        .await
        .map_err(|e| Box::new(crate::CliExitError::new(2, e)) as Box<dyn std::error::Error>)?;
    Ok(true)
}

/// `doctor` / `mcp list` / `mcp serve`：由 [`ExtraCliCommand`] 分流。
async fn try_dispatch_early_extra_cli(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<Option<bool>, Box<dyn std::error::Error>> {
    match d.extra_cli {
        ExtraCliCommand::Doctor => {
            let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
            crate::runtime::cli_doctor::print_doctor_report(&cfg, d.workspace_cli.as_deref());
            Ok(Some(true))
        }
        ExtraCliCommand::McpList { probe } => {
            let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
            crate::runtime::cli_mcp::run_mcp_list(&cfg, probe, false).await;
            Ok(Some(true))
        }
        ExtraCliCommand::McpServe { no_tools, port } => {
            let cfg = load_cli_config_for_early_command(d.config_path, tokens)?;
            crate::runtime::cli_mcp::run_mcp_serve(&cfg, d.workspace_cli, no_tools, port)
                .await
                .map_err(std::io::Error::other)?;
            Ok(Some(true))
        }
        _ => Ok(None),
    }
}

/// `save-session` / `tool-replay` / `plugin *` 等非默认主流程。
fn try_dispatch_early_workspace_commands(
    d: &EarlyCliDispatch<'_>,
    tokens: Option<u32>,
) -> Result<Option<bool>, Box<dyn std::error::Error>> {
    type DispatchFn =
        fn(&EarlyCliDispatch<'_>, Option<u32>) -> Result<bool, Box<dyn std::error::Error>>;
    type DispatchFnNoTokens = fn(&EarlyCliDispatch<'_>) -> Result<bool, Box<dyn std::error::Error>>;
    // 接受 tokens 参数的子命令
    let with_tokens: &[DispatchFn] = &[
        try_early_save_session,
        try_early_tool_replay,
        try_early_plugin_init,
        try_early_plugin_validate,
        try_early_plugin_list,
    ];
    for f in with_tokens {
        if f(d, tokens)? {
            return Ok(Some(true));
        }
    }
    // 不含 tokens 的子命令
    let no_tokens: &[DispatchFnNoTokens] = &[
        try_early_sse_replay,
        try_early_workflow_validate,
        try_early_workflow_compile,
    ];
    for f in no_tokens {
        if f(d)? {
            return Ok(Some(true));
        }
    }
    Ok(None)
}

async fn run_early_commands(
    d: EarlyCliDispatch<'_>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(true) = try_dispatch_early_extra_cli(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    if let Some(true) = try_dispatch_early_workspace_commands(&d, llm_context_tokens_cli)? {
        return Ok(true);
    }
    if try_early_workflow_run(&d, llm_context_tokens_cli).await? {
        return Ok(true);
    }
    Ok(false)
}

async fn run_dry_run(
    config_path: &Option<String>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path.as_deref())?,
        llm_context_tokens_cli,
    );
    let static_dir = web_static_dir::resolve_web_static_dir();
    if !static_dir.is_dir() {
        let msg = format!(
            "dry-run 失败：前端静态目录不存在：{}（请先构建：cd frontend && trunk build）",
            static_dir.display()
        );
        eprintln!("{msg}");
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg).into());
    }
    let key_note = match cfg.llm.llm_http_auth_mode {
        config::LlmHttpAuthMode::None => "llm_http_auth_mode=none（API_KEY 可选）".to_string(),
        config::LlmHttpAuthMode::Bearer => {
            if env::var("API_KEY")
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            {
                "llm_http_auth_mode=bearer 且 API_KEY 非空".to_string()
            } else {
                "llm_http_auth_mode=bearer：当前未检测到非空 API_KEY（可在 Web 侧栏设置或 REPL /api-key 配置后再对话）"
                    .to_string()
            }
        }
    };
    println!(
        "配置检查通过：{}，前端静态目录存在：{}",
        key_note,
        static_dir.display()
    );
    Ok(())
}

async fn run_models_or_probe(
    config_path: &Option<String>,
    extra_cli: ExtraCliCommand,
    llm_context_tokens_cli: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path.as_deref())?,
        llm_context_tokens_cli,
    );
    let api_key = require_api_key_for_cli_models_probe(&cfg)?;
    let client = http_client::build_shared_api_client(&cfg)?;
    if extra_cli == ExtraCliCommand::Models {
        crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
    } else {
        crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
    }
    Ok(())
}

#[cfg(feature = "web")]
pub(super) struct ServeBranchArgs<'a> {
    cfg_holder: &'a config::SharedAgentConfig,
    config_path: &'a Option<String>,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    api_key: String,
    workspace_cli: &'a Option<String>,
    port: u16,
    desktop_ready_json: bool,
    http_bind_host: &'a str,
    no_web: bool,
    process_handles: Arc<crate::process_handles::ProcessHandles>,
}

#[cfg(feature = "web")]
struct ServeRuntimeBuilt {
    uploads_dir: std::path::PathBuf,
    state: Arc<AppState>,
}

#[cfg(feature = "web")]
async fn build_serve_runtime_state(
    cfg_holder: &config::SharedAgentConfig,
    config_path: &Option<String>,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    api_key: String,
    initial_workspace: Option<String>,
    process_handles: Arc<crate::process_handles::ProcessHandles>,
) -> Result<ServeRuntimeBuilt, Box<dyn std::error::Error>> {
    let uploads_dir = std::env::temp_dir().join("crabmate_uploads");
    std::fs::create_dir_all(&uploads_dir).ok();
    let (cq_conc, cq_pending, conv_sqlite, ltm_enabled, ltm_store_path) = {
        let g = cfg_holder.read().await;
        (
            g.chat_queues_cache.chat_queue_max_concurrent,
            g.chat_queues_cache.chat_queue_max_pending,
            g.conversation_persistence
                .conversation_store_sqlite_path
                .clone(),
            g.long_term_memory.long_term_memory_enabled,
            g.long_term_memory
                .long_term_memory_store_sqlite_path
                .clone(),
        )
    };
    let chat_queue = chat_job_queue::ChatJobQueue::new(cq_conc, cq_pending);
    let conversation_backing =
        cli_run_serve::conversation_backing_from_sqlite_path(conv_sqlite.trim())?;
    let long_term_memory = cli_run_serve::serve_long_term_memory_runtime(
        ltm_enabled,
        &conversation_backing,
        ltm_store_path.trim(),
    );
    let sse_stream_hub = std::sync::Arc::new(crate::sse::SseStreamHub::new());
    let chat_queue_job_deps = std::sync::Arc::new(chat_job_queue::WebChatQueueDeps {
        cfg: Arc::clone(cfg_holder),
        api_key: api_key.clone(),
        client: client.clone(),
        tools: tools.clone(),
        chat_queue: chat_queue.clone(),
        long_term_memory: long_term_memory.clone(),
        sse_stream_hub: Arc::clone(&sse_stream_hub),
    });
    Ok(ServeRuntimeBuilt {
        uploads_dir: uploads_dir.clone(),
        state: Arc::new(AppState {
            http: web::AppStateHttpCore {
                cfg: Arc::clone(cfg_holder),
                config_path_for_reload: config_path.clone(),
                api_key,
                client,
                tools,
                workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(
                    initial_workspace,
                )),
                uploads_dir,
            },
            chat: web::AppStateChatRuntime {
                chat_queue,
                chat_queue_job_deps,
            },
            conversation: web::AppStateConversationRuntime {
                conversation_backing: std::sync::Arc::new(tokio::sync::RwLock::new(
                    conversation_backing,
                )),
                conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
            },
            aux: web::AppStateWebAux {
                approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                long_term_memory,
                llm_models_health_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
                sse_stream_hub,
                process_handles: Arc::clone(&process_handles),
                async_chat_jobs: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            },
        }),
    })
}

#[cfg(feature = "web")]
fn parse_bind_ip(http_bind_host: &str) -> Result<std::net::IpAddr, std::io::Error> {
    http_bind_host.parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "无效的 Web 监听地址 {:?}（请使用有效 IP，如 127.0.0.1 或 0.0.0.0）",
                http_bind_host
            ),
        )
    })
}

#[cfg(feature = "web")]
async fn validate_bind_auth(
    cfg_holder: &config::SharedAgentConfig,
    bind_ip: std::net::IpAddr,
) -> Result<bool, Box<dyn std::error::Error>> {
    let (auth_enabled, allow_insec) = cli_run_serve::serve_bind_auth_flags(cfg_holder).await;
    if !bind_ip.is_loopback() && !auth_enabled && !allow_insec {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "当前监听地址为非 loopback（如 0.0.0.0），但未配置 web_api_bearer_token；请设置 [agent].web_api_bearer_token / CM_WEB_API_BEARER_TOKEN，或显式设置 allow_insecure_no_auth_for_non_loopback=true（不安全）",
        )
        .into());
    }
    Ok(auth_enabled)
}

#[cfg(feature = "web")]
pub(super) async fn run_serve_branch(
    args: ServeBranchArgs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ServeBranchArgs {
        cfg_holder,
        config_path,
        client,
        tools,
        api_key,
        workspace_cli,
        port,
        desktop_ready_json,
        http_bind_host,
        no_web,
        process_handles,
    } = args;
    let runtime = build_serve_runtime_state(
        cfg_holder,
        config_path,
        client,
        tools,
        api_key.clone(),
        workspace_cli.clone(),
        process_handles,
    )
    .await?;
    let state = runtime.state;
    let uploads_dir = runtime.uploads_dir;
    let sched_tasks = {
        let g = cfg_holder.read().await;
        g.conversation_persistence.scheduled_agent_tasks.clone()
    };
    web::cron_scheduler::spawn_serve_cron_scheduler(Arc::clone(&state), sched_tasks);
    let static_dir = web_static_dir::resolve_web_static_dir();
    cli_run_serve::serve_require_web_api_bearer_when_enabled(cfg_holder).await?;
    let web_api_bearer_layer_enabled =
        cli_run_serve::serve_web_api_bearer_layer_enabled(cfg_holder).await;
    let app = web::server::build_app(
        state.clone(),
        no_web,
        static_dir,
        uploads_dir.clone(),
        web_api_bearer_layer_enabled,
    );
    let bind_ip = parse_bind_ip(http_bind_host)?;
    let auth_enabled = validate_bind_auth(cfg_holder, bind_ip).await?;
    let addr = std::net::SocketAddr::from((bind_ip, port));
    cli_run_serve::serve_log_startup_health(cfg_holder, workspace_cli, &api_key).await;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    println!("Web 服务已启动");
    println!("  监听: http://{}/", actual_addr);
    if bind_ip.is_unspecified() && !auth_enabled {
        eprintln!(
            "  警告: 正在监听所有网卡（{}），接口无鉴权，请勿在不可信网络暴露",
            actual_addr
        );
    }
    if bind_ip.is_loopback() && !auth_enabled {
        eprintln!(
            "  提示: 未配置 web_api_bearer_token 时，受保护路由不校验 Bearer（见中间件逻辑）；对外或共享网络请设置 CM_WEB_API_BEARER_TOKEN / web_api_bearer_token，并可将 web_api_require_bearer=true 强制启动前须配密钥。浏览器可存 localStorage「crabmate-api-bearer-token」。"
        );
    }
    if bind_ip.is_unspecified() && auth_enabled {
        println!("  安全: 已启用 Web API 鉴权（Authorization: Bearer 或 X-API-Key）");
    }
    if desktop_ready_json {
        let ready = serde_json::json!({
            "event": "web_ready",
            "host": actual_addr.ip().to_string(),
            "port": actual_addr.port(),
            "url": format!("http://{}/", actual_addr),
            "auth_enabled": web_api_bearer_layer_enabled,
        });
        println!("{}", serde_json::to_string(&ready).unwrap_or_default());
    }
    info!(target: "crabmate", "Web 服务监听 addr={}", actual_addr);
    // uploads 自动清理：每 10 分钟执行一次；保留 24h；总容量上限 500MB
    tokio::spawn({
        let dir = uploads_dir.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;
                web::cleanup_uploads_dir(
                    dir.clone(),
                    Duration::from_secs(24 * 3600),
                    500 * 1024 * 1024,
                )
                .await;
            }
        }
    });

    // 优雅关闭：监听 SIGTERM / SIGINT，构建 axum graceful shutdown 信号
    let shutdown_signal = build_serve_shutdown_signal(state.clone());

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;
    Ok(())
}

/// 构建 axum graceful shutdown 信号：监听 SIGTERM/SIGINT → 关闭 ChatJobQueue → 等待 in-flight 完成（最多 30 秒）。
#[cfg(feature = "web")]
fn build_serve_shutdown_signal(
    state: Arc<crate::AppState>,
) -> impl std::future::Future<Output = ()> + Send {
    let shutdown = crate::shutdown::GracefulShutdown::new();
    shutdown.clone().spawn_signal_handler();

    let graceful = shutdown.clone();
    let state = state.clone();
    async move {
        graceful.wait_for_shutdown().await;
        state.chat.chat_queue.shutdown();
        log::info!(target: "crabmate", "等待队列运行中任务完成...");
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let remaining = state.chat.chat_queue.running_count();
        if remaining > 0 {
            log::warn!(target: "crabmate", "优雅关闭超时，仍有 {} 个任务正在运行", remaining);
        }
    }
}

#[cfg(not(feature = "web"))]
pub(super) async fn run_serve_branch(
    _args: ServeBranchArgs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "本 crabmate 二进制未启用 `web` Cargo feature，不支持 `serve`。请使用默认构建或 `cargo build --features web`。",
    )
    .into())
}

#[cfg(not(feature = "web"))]
pub(super) struct ServeBranchArgs<'a> {
    pub cfg_holder: &'a config::SharedAgentConfig,
    pub config_path: &'a Option<String>,
    pub client: reqwest::Client,
    pub tools: Vec<crate::types::Tool>,
    pub api_key: String,
    pub workspace_cli: &'a Option<String>,
    pub port: u16,
    pub desktop_ready_json: bool,
    pub http_bind_host: &'a str,
    pub no_web: bool,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
}

/// `--benchmark` / `--batch`：跑批量评测后返回 `true`（调用方应结束进程）。
pub(super) async fn run_benchmark_batch_if_requested(
    cfg_holder: &config::SharedAgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    bench_args: &config::cli::definitions::BenchmarkCliArgs,
) -> Result<bool, Box<dyn std::error::Error>> {
    if bench_args.benchmark.is_none() && bench_args.batch.is_none() {
        return Ok(false);
    }
    let bench_kind_str = bench_args.benchmark.as_deref().unwrap_or("generic");
    let bench_kind = runtime::benchmark::types::BenchmarkKind::parse(bench_kind_str)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let batch_input = bench_args.batch.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "使用 --benchmark 时必须同时指定 --batch <INPUT.jsonl>",
        )
    })?;
    let batch_output = bench_args
        .batch_output
        .as_deref()
        .unwrap_or("benchmark_results.jsonl");

    let system_prompt_override = match bench_args.system_prompt_file.as_deref() {
        Some(path) => {
            let content = std::fs::read_to_string(path).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("无法读取 bench-system-prompt 文件 {path}: {e}"),
                )
            })?;
            Some(content)
        }
        None => None,
    };

    let batch_cfg = runtime::benchmark::types::BatchRunConfig {
        benchmark: bench_kind,
        input_path: batch_input.to_string(),
        output_path: batch_output.to_string(),
        task_timeout_secs: bench_args.task_timeout,
        max_tool_rounds: bench_args.max_tool_rounds,
        resume_from_existing: bench_args.resume,
        system_prompt_override,
    };

    runtime::benchmark::runner::run_batch(cfg_holder, client, api_key, tools, &batch_cfg).await?;
    Ok(true)
}

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_cli_from_parsed(parse_args()?).await
}

/// 已解析 argv：初始化日志并处理 `doctor` / `save-session` 等早退子命令。
pub(super) async fn run_cli_from_parsed(
    args: ParsedCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    observability::init_tracing_subscriber(
        args.log_file.as_deref().map(std::path::Path::new),
        args.serve_port.is_none(),
    )?;

    if run_early_commands(
        EarlyCliDispatch {
            config_path: &args.config_path,
            workspace_cli: &args.workspace_cli,
            extra_cli: args.extra_cli,
            save_session: args.save_session.clone(),
            tool_replay: args.tool_replay.clone(),
            sse_replay: args.sse_replay.clone(),
            plugin_init: args.plugin_init.clone(),
            plugin_validate: args.plugin_validate.clone(),
            plugin_list: args.plugin_list.clone(),
            workflow_validate: args.workflow_validate.clone(),
            workflow_compile: args.workflow_compile.clone(),
            workflow_run: args.workflow_run.clone(),
        },
        args.llm_context_tokens_cli,
    )
    .await?
    {
        return Ok(());
    }

    Box::pin(run_cli_default_main(args)).await
}

/// 默认主路径：`--dry-run`、`models`/`probe`，或 `serve` / `repl` / `chat` / `tui`。
async fn run_cli_default_main(args: ParsedCliArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.dry_run {
        run_dry_run(&args.config_path, args.llm_context_tokens_cli).await?;
        return Ok(());
    }

    let cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(args.config_path.as_deref())?,
        args.llm_context_tokens_cli,
    );

    if matches!(
        args.extra_cli,
        ExtraCliCommand::Models | ExtraCliCommand::Probe
    ) {
        run_models_or_probe(
            &args.config_path,
            args.extra_cli,
            args.llm_context_tokens_cli,
        )
        .await?;
        return Ok(());
    }

    Box::pin(run_cli_interactive_session(args, cfg)).await
}

/// 载入配置并进入 `serve` / `bench` / `chat` / `tui` / `repl` 分发。
async fn run_cli_interactive_session(
    args: ParsedCliArgs,
    cfg: config::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = cli_run_session::init_cli_session_start(cfg, args.no_tools).await?;

    Box::pin(cli_run_session::run_cli_main_routes(
        cli_run_session::CliDispatchArgs {
            session,
            config_path: args.config_path,
            serve_port: args.serve_port,
            serve_desktop_ready_json: args.serve_desktop_ready_json,
            http_bind_host: args.http_bind_host,
            workspace_cli: args.workspace_cli,
            no_web: args.no_web,
            bench_args: args.bench_args,
            chat_cli: args.chat_cli,
            tui: args.tui,
            no_stream: args.no_stream,
            agent_role_cli: args.agent_role_cli,
        },
    ))
    .await
}
