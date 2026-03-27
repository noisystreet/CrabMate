//! CrabMate 库：DeepSeek Agent、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 `log` + `env_logger` 处理；`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。

mod agent;
mod agent_memory;
mod chat_job_queue;
mod config;
/// Web `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
mod conversation_store;
mod health;
mod http_client;
mod llm;
mod path_workspace;
mod redact;
mod runtime;
mod sse;
mod text_sanitize;
mod tool_registry;
mod tool_result;
mod tools;
mod types;
mod web;

use config::cli::{ExtraCliCommand, init_logging, parse_args};
use log::info;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::sync::mpsc;
use types::Message;

/// Web/CLI/基准测试共用的 `run_agent_turn` 入参（避免长参数列表）。
pub struct RunAgentTurnParams<'a> {
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<config::AgentConfig>,
    pub tools: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub per_flight: Option<std::sync::Arc<chat_job_queue::PerTurnFlight>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 终端 CLI：`run_command` 非白名单时在 stdin 交互确认；Web 传 `None`。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub plain_terminal_stream: bool,
    /// 可选：自定义 [`llm::ChatCompletionsBackend`]；`None` 时使用 OpenAI 兼容 HTTP（与历史行为一致）。
    pub llm_backend: Option<&'a (dyn llm::ChatCompletionsBackend + 'static)>,
    /// 覆盖本回合 `chat/completions` 的 **`temperature`**（`None` 则用 [`config::AgentConfig::temperature`]）。
    pub temperature_override: Option<f32>,
    /// 覆盖本回合请求 JSON 中的 **`seed`**（默认 [`types::LlmSeedOverride::FromConfig`]）。
    pub seed_override: types::LlmSeedOverride,
}

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// `cfg` 建议使用 [`Arc`] 共享（与进程内 Web 服务状态一致），以便工具在 `spawn_blocking` 路径中复用同一份配置而不反复深拷贝。
/// 若提供 out，则流式 content 会通过 out 发送（供 SSE 等使用）；`no_stream` 为 true 时 API 使用 `stream: false`，
/// 有正文则通过 `out` 一次性下发整段。
/// 若 `plain_terminal_stream` 为 `true`（仅 **`runtime::cli`** 应传入）：`render_to_terminal` 且 `out` 为 `None` 时，助手正文以**纯文本**流式（或 `--no-stream` 时整段）写入 stdout，不经 `markdown_to_ansi`。
/// 若 `plain_terminal_stream` 为 `false` 且 `render_to_terminal` 为 `true`：仍在整段到达后用 `markdown_to_ansi` 渲染（用于服务端 jobs 等 **`out.is_none()`** 场景，避免与 CLI 混淆）。
/// 当 `out` 为 `None` 且 `render_to_terminal` 为 `true` 时，分阶段规划通知、分步注入 user 与各工具结果另经 `runtime::terminal_cli_transcript` 写入 stdout；通知与注入正文经 `user_message_for_chat_display`（分步长句可压缩）；`plain_terminal_stream` 为 `true` 时助手正文为上游原始增量/拼接，为 `false` 时经 `assistant_markdown_source_for_display` 管线再渲染。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
/// `cancel` 为 `Some` 时，各轮请求会在流式读与重试间隔中轮询其标志；置位后尽快结束并返回 `Ok`（或 `Err` 与常量 [`crate::types::LLM_CANCELLED_ERROR`] 对齐），供协作取消等场景使用。
/// 分阶段规划（`staged_plan_execution` / `logical_dual_agent`）下若规划轮未解析出合法 `agent_reply_plan` v1：仍经 SSE 下发 `code: staged_plan_invalid`（有 `out` 时），并返回 `Err`（`Display` 以 `staged_plan_invalid:` 前缀开头，便于调用方与队列任务与通用 `INTERNAL_ERROR` 区分）；**不会**把该轮无效规划 assistant 写入 `messages`。
/// `per_flight` 仅 Web 队列任务传入，用于 `GET /status` 的 `per_active_jobs` 镜像；CLI 传 `None`。
/// `llm_backend` 见 [`RunAgentTurnParams::llm_backend`]。
pub async fn run_agent_turn<'a>(
    p: RunAgentTurnParams<'a>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let RunAgentTurnParams {
        client,
        api_key,
        cfg,
        tools,
        messages,
        out,
        effective_working_dir,
        workspace_is_set,
        render_to_terminal,
        no_stream,
        cancel,
        per_flight,
        web_tool_ctx,
        cli_tool_ctx,
        plain_terminal_stream,
        llm_backend,
        temperature_override,
        seed_override,
    } = p;
    let llm_backend: &(dyn llm::ChatCompletionsBackend + 'static) = match llm_backend {
        Some(b) => b,
        None => llm::default_chat_completions_backend(),
    };
    let mut loop_params = agent::agent_turn::RunLoopParams {
        llm_backend,
        client,
        api_key,
        cfg,
        tools_defs: tools,
        messages,
        out,
        effective_working_dir,
        workspace_is_set,
        no_stream,
        cancel: cancel.as_deref(),
        render_to_terminal,
        plain_terminal_stream,
        web_tool_ctx,
        cli_tool_ctx,
        per_flight,
        temperature_override,
        seed_override,
    };
    agent::agent_turn::run_agent_turn_common(&mut loop_params).await
}

pub(crate) use conversation_store::SaveConversationOutcome;
pub(crate) use web::AppState;
pub(crate) use web::save_outcome_to_stream_error_line;

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (
        config_path,
        chat_cli,
        serve_port,
        http_bind_host,
        workspace_cli,
        _output_mode,
        no_tools,
        no_web,
        dry_run,
        no_stream,
        log_file,
        bench_args,
        extra_cli,
    ) = parse_args();

    // 非 Web `--serve` 的 CLI 默认不输出 info（仅 warn+），除非设置 RUST_LOG 或 `--log` 文件（见 `init_logging`）
    init_logging(
        log_file.as_deref().map(std::path::Path::new),
        serve_port.is_none(),
    )?;

    if extra_cli == ExtraCliCommand::Doctor {
        let cfg = match config::load_config(config_path.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}", e);
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e).into());
            }
        };
        crate::runtime::cli_doctor::print_doctor_report(&cfg, workspace_cli.as_deref());
        return Ok(());
    }

    let api_key = match env::var("API_KEY") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("请设置环境变量 API_KEY");
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "未设置环境变量 API_KEY",
            )
            .into());
        }
    };

    let cfg = match config::load_config(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e).into());
        }
    };

    if matches!(extra_cli, ExtraCliCommand::Models | ExtraCliCommand::Probe) {
        let client = http_client::build_shared_api_client(&cfg)?;
        if extra_cli == ExtraCliCommand::Models {
            crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
        } else {
            crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
        }
        return Ok(());
    }

    let cfg = Arc::new(cfg);
    info!(
        target: "crabmate",
        "配置已加载 api_base={} model={}",
        cfg.api_base,
        cfg.model
    );
    if dry_run {
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        if !static_dir.is_dir() {
            let msg = format!(
                "dry-run 失败：前端静态目录不存在：{}（请先在 frontend/ 下构建）",
                static_dir.display()
            );
            eprintln!("{msg}");
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg).into());
        }
        println!(
            "配置检查通过：API_KEY 已设置，配置可用，前端静态目录存在：{}",
            static_dir.display()
        );
        return Ok(());
    }
    let client = http_client::build_shared_api_client(cfg.as_ref())?;
    let all_tools = tools::build_tools();
    let tools = if no_tools { Vec::new() } else { all_tools };

    if let Some(port) = serve_port {
        let initial_workspace = workspace_cli.clone();
        let uploads_dir = std::env::temp_dir().join("crabmate_uploads");
        std::fs::create_dir_all(&uploads_dir).ok();
        let chat_queue = chat_job_queue::ChatJobQueue::new(
            cfg.chat_queue_max_concurrent,
            cfg.chat_queue_max_pending,
        );
        let conversation_backing = if cfg.conversation_store_sqlite_path.trim().is_empty() {
            web::ConversationBacking::memory_default()
        } else {
            let p = std::path::Path::new(cfg.conversation_store_sqlite_path.trim());
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
        let state = Arc::new(AppState {
            cfg: Arc::clone(&cfg),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: uploads_dir.clone(),
            chat_queue,
            conversation_backing,
            conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
            approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        });
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        let app = web::server::build_app(state, no_web, static_dir, uploads_dir.clone());
        let bind_ip: std::net::IpAddr = http_bind_host.parse().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "无效的 Web 监听地址 {:?}（请使用有效 IP，如 127.0.0.1 或 0.0.0.0）",
                    http_bind_host
                ),
            )
        })?;
        let auth_enabled = !cfg.web_api_bearer_token.trim().is_empty();
        if !bind_ip.is_loopback() && !auth_enabled && !cfg.allow_insecure_no_auth_for_non_loopback {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "当前监听地址为非 loopback（如 0.0.0.0），但未配置 web_api_bearer_token；请设置 [agent].web_api_bearer_token / AGENT_WEB_API_BEARER_TOKEN，或显式设置 allow_insecure_no_auth_for_non_loopback=true（不安全）",
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
        if bind_ip.is_unspecified() && auth_enabled {
            println!("  安全: 已启用 Bearer 鉴权（Authorization 头）");
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
        axum::serve(listener, app).await?;
        return Ok(());
    }

    // ---- Benchmark 批量测评模式 ----
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

        runtime::benchmark::runner::run_batch(&cfg, &client, &api_key, &tools, &batch_cfg).await?;
        return Ok(());
    }

    if chat_cli.wants_chat() {
        crate::runtime::cli::run_chat_invocation(
            &cfg,
            &client,
            &api_key,
            &tools,
            &workspace_cli,
            &chat_cli,
        )
        .await?;
        return Ok(());
    }

    crate::runtime::cli::run_repl(&cfg, &client, &api_key, &tools, &workspace_cli, no_stream).await
}

pub use config::{AgentConfig, load_config};
pub use llm::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend,
};
pub use tool_registry::{
    ToolDispatchMeta, ToolExecutionClass, all_dispatch_metadata, execution_class_for_tool,
    is_readonly_tool, try_dispatch_meta,
};
pub use tools::dev_tag;
pub use tools::{ToolsBuildOptions, build_tools, build_tools_filtered, build_tools_with_options};
pub use types::LlmSeedOverride;

pub use runtime::cli_exit::CliExitError;

// 供集成测试或外部二进制引用 `ChatCliArgs` 形状（与 `parse_args` 第二项一致）。
pub use config::cli::ChatCliArgs;

#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
