//! `serve` 分支装配：运行时状态构建、uploads 清理、优雅关闭信号与 `run_serve_branch` 的两个 feature 变体。
//!
//! 从 [`crate::cli_run`] 拆出（S5-a 第二步）：**纯搬移，不改语义**。与 `run_serve_branch` 同族的
//! `parse_bind_ip` / `serve_log_bind_warnings` / `print_desktop_ready_json` 与两个后台定时器
//! （uploads 清理、会话保留兜底）也一并在本模块内：它们只被这里的装配调用，留在 `cli_run.rs`
//! 只会让「serve 起了哪些后台循环」分散在两个文件里。

use std::sync::Arc;

use crate::config;

#[cfg(feature = "web")]
use std::collections::HashMap;
#[cfg(feature = "web")]
use std::net::SocketAddr;
#[cfg(feature = "web")]
use std::sync::atomic::AtomicU64;
#[cfg(feature = "web")]
use std::time::Duration;

#[cfg(feature = "web")]
use log::info;

#[cfg(feature = "web")]
use super::cli_run_serve;
#[cfg(feature = "web")]
use crate::AppState;
#[cfg(feature = "web")]
use crate::chat_job_queue;
#[cfg(feature = "web")]
use crate::web;

/// `serve` 分支的装配入参。
///
/// web 版字段为 `pub(super)`：搬进本模块后结构体不再是 `cli_run` 的同级项，
/// 兄弟模块 `cli_run_session` 仍要逐字段构造它，故不能照抄原先的私有字段。
#[cfg(feature = "web")]
pub(super) struct ServeBranchArgs<'a> {
    pub(super) cfg_holder: &'a config::SharedAgentConfig,
    pub(super) config_path: &'a Option<String>,
    pub(super) client: reqwest::Client,
    pub(super) tools: Vec<crate::types::Tool>,
    pub(super) api_key: String,
    pub(super) workspace_cli: &'a Option<String>,
    pub(super) port: u16,
    pub(super) desktop_ready_json: bool,
    /// 待绑定工作区（`serve --workspace-unset`）：初始 override 置 `None`，等 client `POST /workspace`
    pub(super) workspace_unset: bool,
    pub(super) http_bind_host: &'a str,
    pub(super) process_handles: Arc<crate::process_handles::ProcessHandles>,
}

#[cfg(feature = "web")]
struct ServeRuntimeBuildInput<'a> {
    cfg_holder: &'a config::SharedAgentConfig,
    config_path: &'a Option<String>,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    api_key: String,
    initial_workspace: Option<String>,
    process_handles: Arc<crate::process_handles::ProcessHandles>,
    paths: cli_run_serve::ServeResolvedPaths,
}

#[cfg(feature = "web")]
async fn build_serve_runtime_state(
    input: ServeRuntimeBuildInput<'_>,
) -> Result<Arc<AppState>, Box<dyn std::error::Error>> {
    let ServeRuntimeBuildInput {
        cfg_holder,
        config_path,
        client,
        tools,
        api_key,
        initial_workspace,
        process_handles,
        paths,
    } = input;
    let (cq_conc, cq_pending) = {
        let g = cfg_holder.read().await;
        (
            g.chat_queues_cache.chat_queue_max_concurrent,
            g.chat_queues_cache.chat_queue_max_pending,
        )
    };
    std::fs::create_dir_all(&paths.uploads_dir).ok();
    {
        let mut g = cfg_holder.write().await;
        g.chat_uploads_dir = Some(paths.uploads_dir.clone());
        g.chat_workspace_root = Some(paths.workspace_root.clone());
    }
    let chat_queue = chat_job_queue::ChatJobQueue::new(cq_conc, cq_pending);
    let conversation_backing =
        cli_run_serve::conversation_backing_from_sqlite_path(paths.conversation_sqlite.trim())?;
    let long_term_memory = cli_run_serve::serve_long_term_memory_runtime(
        paths.long_term_memory_enabled,
        &conversation_backing,
        paths.long_term_memory_sqlite.trim(),
    );
    let sse_stream_hub = std::sync::Arc::new(crate::sse::SseStreamHub::new());
    let tool_job_registry = {
        let g = cfg_holder.read().await;
        crate::cm_internal::tool_jobs::registry_from_config(&g)
    };
    // 后台任务 TTL 清理定时器（终态过期 → 轮询 410）；serve 级单例，与注册表同生命周期。
    crate::cm_internal::tool_jobs::spawn_cleanup_task(
        Arc::clone(&tool_job_registry),
        std::time::Duration::from_secs(60),
    );
    let api_key: Arc<str> = Arc::from(api_key);
    let chat_queue_job_deps = std::sync::Arc::new(chat_job_queue::WebChatQueueDeps {
        cfg: Arc::clone(cfg_holder),
        api_key: api_key.to_string(),
        client: client.clone(),
        tools: tools.clone(),
        chat_queue: chat_queue.clone(),
        long_term_memory: long_term_memory.clone(),
        sse_stream_hub: Arc::clone(&sse_stream_hub),
        llm_backend: None,
        turn_runner: crate::default_turn_engine(),
    });
    Ok(Arc::new(AppState {
        http: web::AppStateHttpCore {
            cfg: Arc::clone(cfg_holder),
            config_path_for_reload: config_path.clone(),
            api_key,
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: paths.uploads_dir,
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
            cfg: std::sync::Arc::clone(cfg_holder),
        },
        aux: web::AppStateWebAux {
            approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            long_term_memory,
            llm_models_health_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            tiktoken_baseline_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            sse_stream_hub,
            process_handles: Arc::clone(&process_handles),
            async_chat_jobs: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            tool_job_registry,
        },
    }))
}

/// `--http-bind-host` 解析；错误信息直接面向用户，故保留原文。
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

/// 启动期把「实际监听地址 + 是否启用鉴权」讲清楚：不配 token 不是错误，但必须显式提示。
#[cfg(feature = "web")]
fn serve_log_bind_warnings(bind_ip: std::net::IpAddr, auth_enabled: bool, actual_addr: SocketAddr) {
    if bind_ip.is_unspecified() && !auth_enabled {
        eprintln!(
            "  警告: 正在监听所有网卡（{}），接口无鉴权，请勿在不可信网络暴露",
            actual_addr
        );
    }
    if bind_ip.is_loopback() && !auth_enabled {
        eprintln!(
            "  提示: 未配置 web_api_bearer_token 时，受保护路由不校验 Bearer（见中间件逻辑）；对外或共享网络请设置 CM_WEB_API_BEARER_TOKEN / web_api_bearer_token / `crabmate web-bearer set`，并可将 web_api_require_bearer=true 强制启动前须配密钥。浏览器须在设置中保存同一串 Web API 共享密钥（localStorage「crabmate-api-bearer-token」；勿与模型 API_KEY 混淆）。"
        );
    }
    if bind_ip.is_unspecified() && auth_enabled {
        println!("  安全: 已启用 Web API 鉴权（Authorization: Bearer 或 X-API-Key）");
    }
}

/// `--web-ready-json` / `--desktop-ready-json`：给宿主进程（Desktop / `crabmate ctl`）的单行 JSON。
#[cfg(feature = "web")]
fn print_desktop_ready_json(
    actual_addr: SocketAddr,
    web_api_bearer_layer_enabled: bool,
    instance_id: Option<&str>,
    config: Option<&str>,
) {
    let ready = cli_run_serve::serve_ready_json(
        actual_addr,
        web_api_bearer_layer_enabled,
        instance_id,
        config,
    );
    println!("{}", serde_json::to_string(&ready).unwrap_or_default());
}

#[cfg(feature = "web")]
fn spawn_uploads_cleanup(state: Arc<AppState>) {
    // uploads 自动清理：每 10 分钟执行一次；保留 24h；总容量上限 500MB；会话仍引用的文件跳过
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(600));
        loop {
            interval.tick().await;
            let dir = {
                let g = state.http.cfg.read().await;
                g.chat_uploads_dir.clone()
            };
            let Some(dir) = dir else {
                continue;
            };
            let referenced = state.conversation.referenced_upload_filenames().await;
            web::cleanup_uploads_dir(
                dir,
                Duration::from_secs(24 * 3600),
                500 * 1024 * 1024,
                &referenced,
            )
            .await;
        }
    });
}

/// 会话保留策略的兜底清理：启动时立即执行一次，此后每小时一次；TTL / 条数上限取自配置（可热更）。
#[cfg(feature = "web")]
fn spawn_conversation_prune(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state.conversation.prune_conversations_now().await;
        }
    });
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
        workspace_unset,
        http_bind_host,
        process_handles,
    } = args;
    let cli_run_serve::ServePrecheck {
        instance_id,
        paths,
        // 绑定保留到函数结束，确保持锁覆盖整个进程生命周期。
        instance_locks: _instance_locks,
    } = cli_run_serve::serve_precheck(
        cfg_holder,
        workspace_cli.as_deref(),
        config_path.as_deref(),
    )
    .await?;
    let state = build_serve_runtime_state(ServeRuntimeBuildInput {
        cfg_holder,
        config_path,
        client,
        tools,
        api_key: api_key.clone(),
        initial_workspace: cli_run_serve::initial_workspace_override(workspace_cli, workspace_unset),
        process_handles,
        paths,
    })
    .await?;
    let sched_tasks = {
        let g = cfg_holder.read().await;
        g.conversation_persistence.scheduled_agent_tasks.clone()
    };
    web::cron_scheduler::spawn_serve_cron_scheduler(Arc::clone(&state), sched_tasks);
    cli_run_serve::serve_require_web_api_bearer_when_enabled(cfg_holder).await?;
    let web_api_bearer_layer_enabled =
        cli_run_serve::serve_web_api_bearer_layer_enabled(cfg_holder).await;
    let cors_allowed_origins = {
        let g = cfg_holder.read().await;
        g.web_api.web_cors_allowed_origins.clone()
    };
    let app = web::server::build_app(
        state.clone(),
        web_api_bearer_layer_enabled,
        cors_allowed_origins.clone(),
    );
    let bind_ip = parse_bind_ip(http_bind_host)?;
    let auth_enabled = cli_run_serve::serve_validate_bind_gates(cfg_holder, bind_ip).await?;
    let addr = std::net::SocketAddr::from((bind_ip, port));
    cli_run_serve::serve_log_startup_health(cfg_holder, workspace_cli).await;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    println!("Web 服务已启动");
    println!("  监听: http://{}/", actual_addr);
    serve_log_bind_warnings(bind_ip, auth_enabled, actual_addr);
    cli_run_serve::serve_log_cors_startup(&cors_allowed_origins);
    if desktop_ready_json {
        print_desktop_ready_json(
            actual_addr,
            web_api_bearer_layer_enabled,
            instance_id.as_deref(),
            config_path.as_deref(),
        );
    }
    info!(target: "crabmate", "Web 服务监听 addr={}", actual_addr);
    spawn_uploads_cleanup(state.clone());
    spawn_conversation_prune(state.clone());
    crate::web::spawn_tiktoken_baseline_warmup(state.clone());

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

/// 构建 axum graceful shutdown 信号：监听 SIGTERM/SIGINT（第二次 SIGINT 强退）→ 关闭 ChatJobQueue → 等待 in-flight 完成（最多 30 秒）。
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
    /// 待绑定工作区（`serve --workspace-unset`）：初始 override 置 `None`，等 client `POST /workspace`
    pub workspace_unset: bool,
    pub http_bind_host: &'a str,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
}
