//! `cargo run` / 库入口 [`crate::run`] 的 CLI 编排（从 `lib.rs` 拆出以降低单函数圈复杂度）。

use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use log::info;

use crate::AppState;
use crate::chat_job_queue;
use crate::config;
use crate::config::cli::{
    ExtraCliCommand, ParsedCliArgs, PluginInitCli, PluginListCli, PluginValidateCli,
    SaveSessionCli, parse_args,
};
use crate::http_client;
use crate::observability;
use crate::runtime;
use crate::tool_call_explain;
use crate::tools;
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

struct EarlyCliDispatch<'a> {
    config_path: &'a Option<String>,
    workspace_cli: &'a Option<String>,
    extra_cli: ExtraCliCommand,
    save_session: Option<SaveSessionCli>,
    tool_replay: Option<crate::config::cli::ToolReplayCli>,
    plugin_init: Option<PluginInitCli>,
    plugin_validate: Option<PluginValidateCli>,
    plugin_list: Option<PluginListCli>,
}

async fn run_early_commands(
    d: EarlyCliDispatch<'_>,
    llm_context_tokens_cli: Option<u32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if d.extra_cli == ExtraCliCommand::Doctor {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli_doctor::print_doctor_report(&cfg, d.workspace_cli.as_deref());
        return Ok(true);
    }

    if let ExtraCliCommand::McpList { probe } = d.extra_cli {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli_mcp::run_mcp_list(&cfg, probe, false).await;
        return Ok(true);
    }

    if let ExtraCliCommand::McpServe { no_tools } = d.extra_cli {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli_mcp::run_mcp_serve(&cfg, d.workspace_cli, no_tools)
            .await
            .map_err(std::io::Error::other)?;
        return Ok(true);
    }

    if let Some(ss) = d.save_session {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli::run_save_session_command(&cfg, d.workspace_cli, ss)?;
        return Ok(true);
    }

    if let Some(tr) = d.tool_replay {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli::run_tool_replay_command(&cfg, d.workspace_cli, tr)?;
        return Ok(true);
    }

    if let Some(pi) = d.plugin_init {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli::run_plugin_init_command(&cfg, d.workspace_cli, pi)?;
        return Ok(true);
    }
    if let Some(pv) = d.plugin_validate {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli::run_plugin_validate_command(&cfg, d.workspace_cli, pv)?;
        return Ok(true);
    }
    if let Some(pl) = d.plugin_list {
        let cfg = apply_cli_llm_context_tokens_override(
            config::load_config_for_cli(d.config_path.as_deref())?,
            llm_context_tokens_cli,
        );
        crate::runtime::cli::run_plugin_list_command(&cfg, d.workspace_cli, pl)?;
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

struct ServeBranchArgs<'a> {
    cfg_holder: &'a config::SharedAgentConfig,
    config_path: &'a Option<String>,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    api_key: String,
    workspace_cli: &'a Option<String>,
    port: u16,
    http_bind_host: &'a str,
    no_web: bool,
    process_handles: Arc<crate::process_handles::ProcessHandles>,
}

async fn run_serve_branch(args: ServeBranchArgs<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let ServeBranchArgs {
        cfg_holder,
        config_path,
        client,
        tools,
        api_key,
        workspace_cli,
        port,
        http_bind_host,
        no_web,
        process_handles,
    } = args;
    let initial_workspace = workspace_cli.clone();
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
    let conversation_backing = if conv_sqlite.trim().is_empty() {
        web::ConversationBacking::memory_default()
    } else {
        let p = std::path::Path::new(conv_sqlite.trim());
        let conn = web::open_conversation_sqlite(p).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("无法初始化会话 SQLite {}: {}", p.display(), e),
            )
        })?;
        info!(
            target: "crabmate",
            "Web 会话持久化已启用 path={}",
            p.display()
        );
        web::ConversationBacking::Sqlite(conn)
    };
    let long_term_memory = if ltm_enabled {
        match &conversation_backing {
            web::ConversationBacking::Sqlite(conn) => Some(
                crate::memory::long_term_memory::LongTermMemoryRuntime::new_shared_sqlite(
                    Arc::clone(conn),
                ),
            ),
            web::ConversationBacking::Memory(_) => {
                let p = ltm_store_path.trim();
                if p.is_empty() {
                    info!(
                        target: "crabmate",
                        "长期记忆已启用：Web 会话为内存模式且未配置 long_term_memory_store_sqlite_path，跳过持久化记忆"
                    );
                    None
                } else {
                    match crate::memory::long_term_memory::LongTermMemoryRuntime::open(
                        std::path::Path::new(p),
                    ) {
                        Ok(r) => Some(r),
                        Err(e) => {
                            log::warn!(
                                target: "crabmate",
                                "长期记忆库打开失败 path={} error={}",
                                p,
                                e
                            );
                            None
                        }
                    }
                }
            }
        }
    } else {
        None
    };
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
    let state = Arc::new(AppState {
        http: web::AppStateHttpCore {
            cfg: Arc::clone(cfg_holder),
            config_path_for_reload: config_path.clone(),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: uploads_dir.clone(),
        },
        chat: web::AppStateChatRuntime {
            chat_queue,
            chat_queue_job_deps,
        },
        conversation: web::AppStateConversationRuntime {
            conversation_backing,
            conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
        },
        aux: web::AppStateWebAux {
            approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            long_term_memory,
            web_tasks_by_workspace: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            llm_models_health_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            sse_stream_hub,
            process_handles: Arc::clone(&process_handles),
            async_chat_jobs: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        },
    });
    let sched_tasks = {
        let g = cfg_holder.read().await;
        g.conversation_persistence.scheduled_agent_tasks.clone()
    };
    web::cron_scheduler::spawn_serve_cron_scheduler(Arc::clone(&state), sched_tasks);
    let static_dir = web_static_dir::resolve_web_static_dir();
    {
        let g = cfg_holder.read().await;
        if g.web_api.web_api_require_bearer
            && crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
                .trim()
                .is_empty()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "已启用 web_api_require_bearer（或 CM_WEB_API_REQUIRE_BEARER），但未配置非空的 web_api_bearer_token / CM_WEB_API_BEARER_TOKEN；请设置共享密钥后再启动 serve，或在配置中关闭 web_api_require_bearer。",
            )
            .into());
        }
    }
    let web_api_bearer_layer_enabled = {
        let g = cfg_holder.read().await;
        !crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
            .trim()
            .is_empty()
    };
    let app = web::server::build_app(
        state,
        no_web,
        static_dir,
        uploads_dir.clone(),
        web_api_bearer_layer_enabled,
    );
    let bind_ip: std::net::IpAddr = http_bind_host.parse().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "无效的 Web 监听地址 {:?}（请使用有效 IP，如 127.0.0.1 或 0.0.0.0）",
                http_bind_host
            ),
        )
    })?;
    let (auth_enabled, allow_insec) = {
        let g = cfg_holder.read().await;
        (
            !crate::config::ExposeSecret::expose_secret(&g.web_api.web_api_bearer_token)
                .trim()
                .is_empty(),
            g.web_api.allow_insecure_no_auth_for_non_loopback,
        )
    };
    if !bind_ip.is_loopback() && !auth_enabled && !allow_insec {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "当前监听地址为非 loopback（如 0.0.0.0），但未配置 web_api_bearer_token；请设置 [agent].web_api_bearer_token / CM_WEB_API_BEARER_TOKEN，或显式设置 allow_insecure_no_auth_for_non_loopback=true（不安全）",
        )
        .into());
    }
    let addr = std::net::SocketAddr::from((bind_ip, port));
    println!("Web 服务已启动");
    println!("  监听: http://{}/", addr);
    if bind_ip.is_unspecified() && !auth_enabled {
        eprintln!(
            "  警告: 正在监听所有网卡（{}），接口无鉴权，请勿在不可信网络暴露",
            addr
        );
    }
    if bind_ip.is_loopback() && !auth_enabled {
        eprintln!(
            "  提示: 未配置 web_api_bearer_token，受保护 API 可被本机任意进程调用。嵌入默认已启用 web_api_require_bearer；若需纯本地匿名调试请在配置中设 web_api_require_bearer = false 并（可选）清空密钥。否则请设置 CM_WEB_API_BEARER_TOKEN（及浏览器 localStorage「API」同源键 crabmate-api-bearer-token）。"
        );
    }
    if bind_ip.is_unspecified() && auth_enabled {
        println!("  安全: 已启用 Web API 鉴权（Authorization: Bearer 或 X-API-Key）");
    }
    info!(target: "crabmate", "Web 服务监听 addr={}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ParsedCliArgs {
        config_path,
        agent_role_cli,
        chat_cli,
        serve_port,
        http_bind_host,
        workspace_cli,
        no_tools,
        no_web,
        dry_run,
        no_stream,
        log_file,
        bench_args,
        extra_cli,
        save_session,
        tool_replay,
        plugin_init,
        plugin_validate,
        plugin_list,
        llm_context_tokens_cli,
    } = parse_args()?;

    observability::init_tracing_subscriber(
        log_file.as_deref().map(std::path::Path::new),
        serve_port.is_none(),
    )?;

    if run_early_commands(
        EarlyCliDispatch {
            config_path: &config_path,
            workspace_cli: &workspace_cli,
            extra_cli,
            save_session,
            tool_replay,
            plugin_init,
            plugin_validate,
            plugin_list,
        },
        llm_context_tokens_cli,
    )
    .await?
    {
        return Ok(());
    }

    if dry_run {
        run_dry_run(&config_path, llm_context_tokens_cli).await?;
        return Ok(());
    }

    let cfg = apply_cli_llm_context_tokens_override(
        config::load_config_for_cli(config_path.as_deref())?,
        llm_context_tokens_cli,
    );

    if matches!(extra_cli, ExtraCliCommand::Models | ExtraCliCommand::Probe) {
        run_models_or_probe(&config_path, extra_cli, llm_context_tokens_cli).await?;
        return Ok(());
    }

    let api_key = read_llm_api_key_from_env_lenient(&cfg);

    let cfg_holder: config::SharedAgentConfig = std::sync::Arc::new(tokio::sync::RwLock::new(cfg));
    {
        let g = cfg_holder.read().await;
        info!(
            target: "crabmate",
            "配置已加载 api_base={} model={}",
            g.llm.api_base,
            g.llm.model
        );
    }
    let client = {
        let g = cfg_holder.read().await;
        http_client::build_shared_api_client(&g)?
    };
    let mut all_tools = tools::build_tools();
    {
        let g = cfg_holder.read().await;
        tool_call_explain::annotate_tool_defs_for_explain_card(&mut all_tools, &g);
    }
    let tools = if no_tools { Vec::new() } else { all_tools };

    let cli_process_handles = crate::process_handles::ProcessHandles::new_arc(
        Arc::new(crate::workspace::changelist::WorkspaceChangelistRegistry::default()),
        Arc::new(crate::tool_stats::ToolOutcomeRecorder::new()),
        crate::tool_registry::HandlerLookupTable::default_dispatch(),
        crate::tool_sandbox::default_sync_default_sandbox_backend(),
    );

    if let Some(port) = serve_port {
        run_serve_branch(ServeBranchArgs {
            cfg_holder: &cfg_holder,
            config_path: &config_path,
            client,
            tools,
            api_key,
            workspace_cli: &workspace_cli,
            port,
            http_bind_host: &http_bind_host,
            no_web,
            process_handles: Arc::clone(&cli_process_handles),
        })
        .await?;
        return Ok(());
    }

    if bench_args.benchmark.is_some() || bench_args.batch.is_some() {
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

        runtime::benchmark::runner::run_batch(&cfg_holder, &client, &api_key, &tools, &batch_cfg)
            .await?;
        return Ok(());
    }

    if chat_cli.wants_chat() {
        crate::runtime::cli::run_chat_invocation(
            crate::runtime::cli::CliMainInvocationCommon {
                cfg_holder: &cfg_holder,
                config_path: config_path.as_deref(),
                client: &client,
                api_key: &api_key,
                tools: &tools,
                workspace_cli: &workspace_cli,
                agent_role: agent_role_cli.as_deref(),
                process_handles: Arc::clone(&cli_process_handles),
            },
            &chat_cli,
        )
        .await?;
        return Ok(());
    }

    crate::runtime::cli::run_repl(
        crate::runtime::cli::CliMainInvocationCommon {
            cfg_holder: &cfg_holder,
            config_path: config_path.as_deref(),
            client: &client,
            api_key: &api_key,
            tools: &tools,
            workspace_cli: &workspace_cli,
            agent_role: agent_role_cli.as_deref(),
            process_handles: Arc::clone(&cli_process_handles),
        },
        no_stream,
    )
    .await
}
