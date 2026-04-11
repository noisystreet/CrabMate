//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 **`tracing`** + **`tracing-subscriber`** 处理，**`tracing-log`** 桥接既有 `log::` 调用；`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。设 **`AGENT_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

pub mod agent;
mod agent_errors;
mod agent_memory;
/// Web/CLI 多角色工作台：中途切换 system、按角色裁剪工具列表。
mod agent_role_turn;
/// 工作区内 `cargo metadata` 子进程参数单一真源（工具与首轮注入等共用）。
mod cargo_metadata;
mod chat_job_queue;
mod clarification_questionnaire;
/// 工作区代码语义索引与 `codebase_semantic_search` 工具（SQLite + fastembed）。
mod codebase_semantic_index;
mod codebase_semantic_invalidation;
mod config;
/// Web `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
mod conversation_store;
/// Web `/chat*` 与 CLI 首轮项目画像 / 依赖摘要注入的共用拼装。
mod conversation_turn_bootstrap;
mod health;
mod http_client;
mod living_docs;
mod llm;
mod long_term_memory;
mod long_term_memory_store;
mod mcp;
mod observability;
mod path_workspace;
mod project_dependency_brief;
mod project_profile;
mod read_file_turn_cache;
mod redact;
mod request_chrome_trace;
mod runtime;
mod sse;
mod text_encoding;
mod text_sanitize;
mod text_util;
mod tool_approval;
mod tool_call_explain;
mod tool_registry;
mod tool_result;
pub mod tool_sandbox;
mod tool_stats;
mod tools;
mod types;
mod user_message_file_refs;
mod web;
mod web_static_dir;
mod workspace_changelist;
mod workspace_fs;

pub use config::cli::{
    ChatCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionFormat, ToolReplayCli,
    normalize_legacy_argv, parse_args, parse_args_from_argv, root_clap_command_for_man_page,
};
use log::info;
pub use read_file_turn_cache::{ReadFileTurnCache, ReadFileTurnCacheHandle, new_turn_cache_handle};
use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::Instrument;
use types::Message;

/// `crabmate models` / `crabmate probe`：`bearer` 时仍要求进程环境变量 **`API_KEY`** 非空。
fn require_api_key_for_cli_models_probe(
    cfg: &config::AgentConfig,
) -> Result<String, std::io::Error> {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
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
/// Web 可在侧栏「设置」填写密钥（`client_llm.api_key`）；REPL 可用 **`/api-key set …`** 写入本进程内存。
fn read_llm_api_key_from_env_lenient(cfg: &config::AgentConfig) -> String {
    let v = env::var("API_KEY").unwrap_or_default();
    if cfg.llm_http_auth_mode == config::LlmHttpAuthMode::Bearer && v.trim().is_empty() {
        info!(
            target: "crabmate",
            "API_KEY 未设置（llm_http_auth_mode=bearer）：Web 请在侧栏设置中填写 API 密钥；REPL 请使用 /api-key set <密钥>"
        );
    }
    v
}

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
    /// 长期记忆（可选）；与 `long_term_memory_scope_id` 配对使用。
    pub long_term_memory: Option<std::sync::Arc<long_term_memory::LongTermMemoryRuntime>>,
    /// 记忆作用域（如 Web `conversation_id` 或 CLI `cli`）。
    pub long_term_memory_scope_id: Option<String>,
    /// 单轮 `run_agent_turn` 内 `read_file` 结果缓存；`None` 时由 `run_agent_turn` 按配置创建或关闭。
    pub read_file_turn_cache: Option<std::sync::Arc<ReadFileTurnCache>>,
    /// 多角色工作台：本回合允许的工具名；`None` 表示不额外限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// Web `/chat*`：与 **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的结构化日志根 span；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<observability::TracingChatTurn>>,
}

impl<'a> RunAgentTurnParams<'a> {
    /// Web `/chat/stream`：SSE 输出、可选工具审批、可取消。
    #[allow(clippy::too_many_arguments)]
    pub fn web_chat_stream(
        client: &'a reqwest::Client,
        api_key: &'a str,
        cfg: &'a Arc<config::AgentConfig>,
        tools: &'a [crate::types::Tool],
        messages: &'a mut Vec<Message>,
        effective_working_dir: &'a std::path::Path,
        workspace_is_set: bool,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        per_flight: std::sync::Arc<chat_job_queue::PerTurnFlight>,
        web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
        temperature_override: Option<f32>,
        seed_override: types::LlmSeedOverride,
        long_term_memory: Option<std::sync::Arc<long_term_memory::LongTermMemoryRuntime>>,
        job_id: u64,
        conversation_id: &str,
        out: &'a mpsc::Sender<String>,
        turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    ) -> Self {
        Self {
            client,
            api_key,
            cfg,
            tools,
            messages,
            out: Some(out),
            effective_working_dir,
            workspace_is_set,
            render_to_terminal: false,
            no_stream: false,
            cancel: Some(cancel),
            per_flight: Some(per_flight),
            web_tool_ctx,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            llm_backend: None,
            temperature_override,
            seed_override,
            long_term_memory,
            long_term_memory_scope_id: Some(conversation_id.to_string()),
            read_file_turn_cache: None,
            turn_allowed_tool_names,
            tracing_chat_turn: Some(observability::TracingChatTurn::new(job_id, conversation_id)),
        }
    }

    /// Web `POST /chat`（JSON）：无 SSE，终端渲染管线用于分步通知等。
    #[allow(clippy::too_many_arguments)]
    pub fn web_chat_json(
        client: &'a reqwest::Client,
        api_key: &'a str,
        cfg: &'a Arc<config::AgentConfig>,
        tools: &'a [crate::types::Tool],
        messages: &'a mut Vec<Message>,
        effective_working_dir: &'a std::path::Path,
        workspace_is_set: bool,
        per_flight: std::sync::Arc<chat_job_queue::PerTurnFlight>,
        temperature_override: Option<f32>,
        seed_override: types::LlmSeedOverride,
        long_term_memory: Option<std::sync::Arc<long_term_memory::LongTermMemoryRuntime>>,
        job_id: u64,
        conversation_id: &str,
        turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    ) -> Self {
        Self {
            client,
            api_key,
            cfg,
            tools,
            messages,
            out: None,
            effective_working_dir,
            workspace_is_set,
            render_to_terminal: true,
            no_stream: false,
            cancel: None,
            per_flight: Some(per_flight),
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            llm_backend: None,
            temperature_override,
            seed_override,
            long_term_memory,
            long_term_memory_scope_id: Some(conversation_id.to_string()),
            read_file_turn_cache: None,
            turn_allowed_tool_names,
            tracing_chat_turn: Some(observability::TracingChatTurn::new(job_id, conversation_id)),
        }
    }

    /// `chat` 子命令等：本机终端、纯文本流式、可选 `run_command` 交互。
    #[allow(clippy::too_many_arguments)]
    pub fn cli_terminal_chat(
        client: &'a reqwest::Client,
        api_key: &'a str,
        cfg: &'a Arc<config::AgentConfig>,
        tools: &'a [crate::types::Tool],
        messages: &'a mut Vec<Message>,
        effective_working_dir: &'a std::path::Path,
        no_stream: bool,
        cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
        long_term_memory: Option<std::sync::Arc<long_term_memory::LongTermMemoryRuntime>>,
        long_term_memory_scope_id: Option<String>,
        turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    ) -> Self {
        Self {
            client,
            api_key,
            cfg,
            tools,
            messages,
            out: None,
            effective_working_dir,
            workspace_is_set: true,
            render_to_terminal: true,
            no_stream,
            cancel: None,
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx,
            plain_terminal_stream: true,
            llm_backend: None,
            temperature_override: None,
            seed_override: types::LlmSeedOverride::default(),
            long_term_memory,
            long_term_memory_scope_id,
            read_file_turn_cache: None,
            turn_allowed_tool_names,
            tracing_chat_turn: None,
        }
    }

    /// `bench` 批量任务：无终端渲染、非流式、可超时取消。
    pub fn benchmark_batch(
        client: &'a reqwest::Client,
        api_key: &'a str,
        cfg: &'a Arc<config::AgentConfig>,
        tools: &'a [crate::types::Tool],
        messages: &'a mut Vec<Message>,
        effective_working_dir: &'a std::path::Path,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            client,
            api_key,
            cfg,
            tools,
            messages,
            out: None,
            effective_working_dir,
            workspace_is_set: true,
            render_to_terminal: false,
            no_stream: true,
            cancel: Some(cancel),
            per_flight: None,
            web_tool_ctx: None,
            cli_tool_ctx: None,
            plain_terminal_stream: false,
            llm_backend: None,
            temperature_override: None,
            seed_override: types::LlmSeedOverride::default(),
            long_term_memory: None,
            long_term_memory_scope_id: None,
            read_file_turn_cache: None,
            turn_allowed_tool_names: None,
            tracing_chat_turn: None,
        }
    }
}

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// `cfg` 建议使用 [`Arc`] 共享（与进程内 Web 服务状态一致），以便工具在 `spawn_blocking` 路径中复用同一份配置而不反复深拷贝。
/// 若提供 out，则流式 content 会通过 out 发送（供 SSE 等使用）；`no_stream` 为 true 时 API 使用 `stream: false`，
/// 有正文则通过 `out` 一次性下发整段。
/// 若 `plain_terminal_stream` 为 `true`（仅 **`runtime::cli`** 应传入）：`render_to_terminal` 且 `out` 为 `None` 时，助手正文以**纯文本**流式（或 `--no-stream` 时整段）写入 stdout，不经 `markdown_to_ansi`。
/// 若 `plain_terminal_stream` 为 `false` 且 `render_to_terminal` 为 `true`：仍在整段到达后用 `markdown_to_ansi` 渲染（用于服务端 jobs 等 **`out.is_none()`** 场景，避免与 CLI 混淆）。
/// 当 `out` 为 `None` 且 `render_to_terminal` 为 `true` 时，分阶段规划通知、分步注入 user 与各工具结果另经 `runtime::terminal_cli_transcript` 写入 stdout；通知与注入正文经 `user_message_for_chat_display`（分步长句可压缩）；`plain_terminal_stream` 为 `true` 时助手正文为上游原始增量/拼接，为 `false` 时经 `assistant_markdown_source_for_display` 管线再渲染。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
/// `cancel` 为 `Some` 时，各轮请求会在流式读与重试间隔中轮询其标志；置位后尽快结束并返回 `Ok`（或 `Err`：[`agent::agent_turn::RunAgentTurnError`] 中含取消 / 限流 / SSE 早停等，用户可见串与常量 [`crate::types::LLM_CANCELLED_ERROR`] 对齐），供协作取消等场景使用。
/// 分阶段规划（`staged_plan_execution` / `logical_dual_agent`）下若规划轮未解析出合法 `agent_reply_plan` v1：**不再**整轮失败退出：保留规划轮助手正文并**降级**为与关闭分阶段规划时相同的常规 `run_agent_outer_loop`（含工具）。规划轮会先丢弃 API 返回的原生 `tool_calls`，再从正文 DeepSeek DSML 物化并视情况执行工具，避免网关误报 `tool_calls` 时 CLI 静默无动作。
/// `per_flight` 仅 Web 队列任务传入，用于 `GET /status` 的 `per_active_jobs` 镜像；CLI 传 `None`。
/// `llm_backend` 见 [`RunAgentTurnParams::llm_backend`]。
pub async fn run_agent_turn<'a>(
    p: RunAgentTurnParams<'a>,
) -> Result<(), crate::agent::agent_turn::RunAgentTurnError> {
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
        long_term_memory,
        long_term_memory_scope_id,
        read_file_turn_cache,
        turn_allowed_tool_names,
        tracing_chat_turn,
    } = p;
    let llm_backend: &(dyn llm::ChatCompletionsBackend + 'static) = match llm_backend {
        Some(b) => b,
        None => llm::default_chat_completions_backend(),
    };

    let read_file_turn_cache = match read_file_turn_cache {
        Some(a) => Some(a),
        None if cfg.read_file_turn_cache_max_entries > 0 => Some(
            read_file_turn_cache::new_turn_cache_handle(cfg.read_file_turn_cache_max_entries),
        ),
        None => None,
    };

    let workspace_changelist = if cfg.session_workspace_changelist_enabled {
        let scope = long_term_memory_scope_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("__default__");
        Some(crate::workspace_changelist::changelist_for_scope(scope))
    } else {
        None
    };

    let mut tools_for_turn: Vec<types::Tool> = tools.to_vec();
    let mcp_session = match mcp::try_open_session_and_tools(cfg.as_ref()).await {
        Some((sess, extra)) => {
            tools_for_turn = mcp::merge_tool_lists(tools_for_turn, extra);
            Some(sess)
        }
        None => None,
    };
    if !cfg.codebase_semantic_search_enabled {
        tools_for_turn.retain(|t| t.function.name != "codebase_semantic_search");
    }
    if !cfg.long_term_memory_enabled {
        tools_for_turn.retain(|t| {
            !matches!(
                t.function.name.as_str(),
                "long_term_remember" | "long_term_forget" | "long_term_memory_list"
            )
        });
    }
    if let Some(ref allow) = turn_allowed_tool_names {
        let mcp_ok = allow.contains("mcp");
        tools_for_turn.retain(|t| {
            let n = t.function.name.as_str();
            if n.starts_with("mcp__") {
                return mcp_ok;
            }
            allow.contains(n)
        });
    }

    let request_chrome_trace = crate::request_chrome_trace::request_trace_dir_from_env()
        .map(|_| std::sync::Arc::new(crate::request_chrome_trace::RequestTurnTrace::new()));
    let wall_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut loop_params = agent::agent_turn::RunLoopParams {
        llm_backend,
        client,
        api_key,
        cfg,
        tools_defs: tools_for_turn.as_slice(),
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
        long_term_memory,
        long_term_memory_scope_id,
        mcp_session,
        read_file_turn_cache,
        workspace_changelist,
        staged_plan_optimizer_round: cfg.staged_plan_optimizer_round,
        staged_plan_optimizer_requires_parallel_tools: cfg
            .staged_plan_optimizer_requires_parallel_tools,
        staged_plan_ensemble_count: cfg.staged_plan_ensemble_count,
        staged_plan_skip_ensemble_on_casual_prompt: cfg.staged_plan_skip_ensemble_on_casual_prompt,
        request_chrome_trace: request_chrome_trace.clone(),
        step_executor_constraint: None,
        turn_allowed_tool_names: turn_allowed_tool_names.clone(),
        tracing_chat_turn: tracing_chat_turn.clone(),
        sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
    };

    let trace_span = loop_params
        .tracing_chat_turn
        .as_ref()
        .map(|t| t.span.clone());
    let run_common = agent::agent_turn::run_agent_turn_common(&mut loop_params);
    match (trace_span, request_chrome_trace) {
        (Some(span), Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common.instrument(span))
                .await
        }
        (Some(span), None) => run_common.instrument(span).await,
        (None, Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common).await
        }
        (None, None) => run_common.await,
    }
}

pub(crate) use conversation_store::SaveConversationOutcome;
pub(crate) use web::AppState;
pub(crate) use web::conversation_conflict_sse_line;

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
    } = parse_args()?;

    // 非 Web `--serve` 的 CLI 默认不输出 info（仅 warn+），除非设置 RUST_LOG 或 `--log` 文件（见 `init_tracing_subscriber`）
    observability::init_tracing_subscriber(
        log_file.as_deref().map(std::path::Path::new),
        serve_port.is_none(),
    )?;

    if extra_cli == ExtraCliCommand::Doctor {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        crate::runtime::cli_doctor::print_doctor_report(&cfg, workspace_cli.as_deref());
        return Ok(());
    }

    if let ExtraCliCommand::McpList { probe } = extra_cli {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        crate::runtime::cli_mcp::run_mcp_list(&cfg, probe, false).await;
        return Ok(());
    }

    if let ExtraCliCommand::McpServe { no_tools } = extra_cli {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        crate::runtime::cli_mcp::run_mcp_serve(&cfg, &workspace_cli, no_tools).await?;
        return Ok(());
    }

    if let Some(ss) = save_session {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        crate::runtime::cli::run_save_session_command(&cfg, &workspace_cli, ss)?;
        return Ok(());
    }

    if let Some(tr) = tool_replay {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        crate::runtime::cli::run_tool_replay_command(&cfg, &workspace_cli, tr)?;
        return Ok(());
    }

    // `config` 子命令仅做 dry-run 自检，不要求 API_KEY（与 llm_http_auth_mode 一致）
    if dry_run {
        let cfg = config::load_config_for_cli(config_path.as_deref())?;
        let static_dir = web_static_dir::resolve_web_static_dir();
        if !static_dir.is_dir() {
            let msg = format!(
                "dry-run 失败：前端静态目录不存在：{}（请先构建：cd frontend-leptos && trunk build）",
                static_dir.display()
            );
            eprintln!("{msg}");
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg).into());
        }
        let key_note = match cfg.llm_http_auth_mode {
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
        return Ok(());
    }

    let cfg = config::load_config_for_cli(config_path.as_deref())?;

    if matches!(extra_cli, ExtraCliCommand::Models | ExtraCliCommand::Probe) {
        let api_key = require_api_key_for_cli_models_probe(&cfg)?;
        let client = http_client::build_shared_api_client(&cfg)?;
        if extra_cli == ExtraCliCommand::Models {
            crate::runtime::cli_doctor::run_models_cli(&client, &cfg, api_key.trim()).await?;
        } else {
            crate::runtime::cli_doctor::run_probe_cli(&client, &cfg, api_key.trim()).await?;
        }
        return Ok(());
    }

    let api_key = read_llm_api_key_from_env_lenient(&cfg);

    let cfg_holder: config::SharedAgentConfig = std::sync::Arc::new(tokio::sync::RwLock::new(cfg));
    {
        let g = cfg_holder.read().await;
        info!(
            target: "crabmate",
            "配置已加载 api_base={} model={}",
            g.api_base,
            g.model
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

    if let Some(port) = serve_port {
        let initial_workspace = workspace_cli.clone();
        let uploads_dir = std::env::temp_dir().join("crabmate_uploads");
        std::fs::create_dir_all(&uploads_dir).ok();
        let (cq_conc, cq_pending, conv_sqlite, ltm_enabled, ltm_store_path) = {
            let g = cfg_holder.read().await;
            (
                g.chat_queue_max_concurrent,
                g.chat_queue_max_pending,
                g.conversation_store_sqlite_path.clone(),
                g.long_term_memory_enabled,
                g.long_term_memory_store_sqlite_path.clone(),
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
                    long_term_memory::LongTermMemoryRuntime::new_shared_sqlite(Arc::clone(conn)),
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
                        match long_term_memory::LongTermMemoryRuntime::open(std::path::Path::new(p))
                        {
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
        let state = Arc::new(AppState {
            cfg: Arc::clone(&cfg_holder),
            config_path_for_reload: config_path.clone(),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: uploads_dir.clone(),
            chat_queue,
            conversation_backing,
            conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
            approval_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            long_term_memory,
            web_tasks_by_workspace: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            llm_models_health_cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
            sse_stream_hub: std::sync::Arc::new(crate::sse::SseStreamHub::new()),
        });
        let static_dir = web_static_dir::resolve_web_static_dir();
        let web_api_bearer_layer_enabled = {
            let g = cfg_holder.read().await;
            !crate::config::ExposeSecret::expose_secret(&g.web_api_bearer_token)
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
                !crate::config::ExposeSecret::expose_secret(&g.web_api_bearer_token)
                    .trim()
                    .is_empty(),
                g.allow_insecure_no_auth_for_non_loopback,
            )
        };
        if !bind_ip.is_loopback() && !auth_enabled && !allow_insec {
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

        runtime::benchmark::runner::run_batch(&cfg_holder, &client, &api_key, &tools, &batch_cfg)
            .await?;
        return Ok(());
    }

    if chat_cli.wants_chat() {
        crate::runtime::cli::run_chat_invocation(
            &cfg_holder,
            config_path.as_deref(),
            &client,
            &api_key,
            &tools,
            &workspace_cli,
            &chat_cli,
            agent_role_cli.as_deref(),
        )
        .await?;
        return Ok(());
    }

    crate::runtime::cli::run_repl(
        &cfg_holder,
        config_path.as_deref(),
        &client,
        &api_key,
        &tools,
        &workspace_cli,
        no_stream,
        agent_role_cli.as_deref(),
    )
    .await
}

pub use config::{
    AgentConfig, ExposeSecret, LlmHttpAuthMode, SharedAgentConfig, load_config, load_config_for_cli,
};
pub use llm::{
    ChatCompletionsBackend, CompleteChatRetryingParams, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    StreamChatParams, default_chat_completions_backend,
};
pub use tool_registry::{
    ToolDispatchMeta, ToolExecutionClass, all_dispatch_metadata, execution_class_for_tool,
    is_readonly_tool, try_dispatch_meta,
};
pub use tools::dev_tag;
pub use tools::{ToolsBuildOptions, build_tools, build_tools_filtered, build_tools_with_options};
pub use types::LlmSeedOverride;

pub use runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT,
    EXIT_TOOL_REPLAY_MISMATCH, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
    classify_model_error_message,
};

#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
