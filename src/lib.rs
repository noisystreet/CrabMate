//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 **`tracing`** + **`tracing-subscriber`** 处理，**`tracing-log`** 桥接既有 `log::` 调用；`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

pub mod agent;
mod agent_errors;
/// Web/CLI 多角色工作台：中途切换 system、按角色裁剪工具列表。
mod agent_role_turn;
/// Web/CLI 共用的 `run_agent_turn` 前置逻辑（工具合并、MCP、缓存句柄）。
mod agent_turn_prep;
/// 工作区内 `cargo metadata` 子进程参数单一真源（工具与首轮注入等共用）。
mod cargo_metadata;
mod chat_job_queue;
mod clarification_questionnaire;
mod cli_run;
mod config;
/// Web `/chat*` 与 CLI 首轮 living docs / 项目画像 / 依赖摘要与会话 bootstrap。
mod context_bootstrap;
/// Web `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
mod conversation_store;
mod dynamic_tools;
mod health;
mod http_client;
mod llm;
mod mcp;
/// 长期记忆、备忘片段、代码语义索引（SQLite + fastembed）。
mod memory;
/// 元对话门控补充（如「我刚才问了什么」类追问）。
mod meta_dialogue;
mod observability;
mod process_handles;
pub use process_handles::ProcessHandles;

/// 仅 **`cargo test`**：清空 **`run_command`** 全局限流状态与 **`test_result_cache`** LRU，减轻测试顺序依赖。
#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    crate::tools::reset_process_tool_globals_for_tests();
    crate::turn_replay_dump::reset_turn_replay_globals_for_tests();
}

mod read_file_turn_cache;
mod readonly_tool_ttl_cache;
mod redact;
mod request_chrome_trace;
mod runtime;
mod sse;
mod text_encoding;
mod text_sanitize;
mod text_sanitize_dsml_vendor;
mod text_util;
mod tool_approval;
mod tool_call_explain;
mod tool_registry;
mod tool_result;
pub mod tool_sandbox;
mod tool_stats;
mod tools;
mod turn_replay_dump;
mod types;
mod user_message_file_refs;
mod web;
mod web_static_dir;
/// 工作区路径、根内打开（Unix `openat2`）与会话变更集。
mod workspace;

pub use config::cli::{
    ChatCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionFormat, ToolReplayCli,
    normalize_legacy_argv, parse_args, parse_args_from_argv, root_clap_command_for_man_page,
};
pub use read_file_turn_cache::{ReadFileTurnCache, ReadFileTurnCacheHandle, new_turn_cache_handle};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::Instrument;

/// 回合传输与端点表现（SSE、取消、审批上下文、终端渲染等），与模型采样/路由覆盖解耦。
pub struct AgentTurnTransport<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub per_flight: Option<std::sync::Arc<chat_job_queue::PerTurnFlight>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 终端 CLI：`run_command` 非白名单时在 stdin 交互确认；Web 传 `None`。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub plain_terminal_stream: bool,
    /// 全屏 TUI：流式助手增量写入（见 [`runtime::tui::TuiLlmStreamScratch`]）；其它入口 `None`。
    pub tui_llm_stream_scratch: Option<runtime::tui::TuiLlmStreamScratchArc>,
    /// 无 SSE（`out` 为 `None`）时可选：工具批开始/结束时各调用一次（`true` / `false`），与 Web `SsePayload::ToolRunning` 对齐（如 TUI 底栏）。
    pub tool_running_hook: Option<std::sync::Arc<dyn Fn(bool) + Send + Sync>>,
    /// 澄清问卷回调（与 [`crate::agent::agent_turn::RunLoopIo::clarification_questionnaire_hook`] 同源）；Web/SSE 通常为 `None`。
    pub clarification_questionnaire_hook:
        Option<std::sync::Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    /// 无 `/chat/stream` 通道时镜像 SSE 控制面（与 Web [`crate::sse::SsePayload`] 同形），供 TUI 等终端界面展示。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 可选：自定义 [`llm::ChatCompletionsBackend`]；`None` 时使用 OpenAI 兼容 HTTP（与历史行为一致）。
    pub llm_backend: Option<&'a (dyn llm::ChatCompletionsBackend + 'static)>,
}

/// 本回合对 `chat/completions` 的采样与模型路由覆盖（相对 [`config::AgentConfig`]）。
pub struct AgentTurnLlmOverrides {
    /// 覆盖本回合 `chat/completions` 的 **`temperature`**（`None` 则用 [`config::AgentConfig::temperature`]）。
    pub temperature_override: Option<f32>,
    /// 覆盖本回合的 `model`（planner 阶段，见编排层 `use_executor_model`）
    pub model_override: Option<String>,
    /// 若为 `true`，LLM 调用时使用 `cfg.llm.executor_model` 而非 `cfg.llm.planner_model`。
    pub use_executor_model: bool,
    /// 执行阶段模型覆盖（当 use_executor_model 为 true 时优先于 cfg.llm.executor_model）
    pub executor_model_override: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_base。
    pub executor_api_base: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_key。
    pub executor_api_key: Option<String>,
    pub seed_override: types::LlmSeedOverride,
}

/// Web/CLI/bench 共用的 LLM 接入侧不变输入（HTTP 客户端、密钥、配置快照、工具表）。
pub struct RunAgentTurnSharedInputs<'a> {
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<config::AgentConfig>,
    pub tools: &'a [crate::types::Tool],
}

/// Web/CLI/基准测试共用的 `run_agent_turn` 入参（避免长参数列表）。
pub struct RunAgentTurnParams<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub transport: AgentTurnTransport<'a>,
    pub llm: AgentTurnLlmOverrides,
    /// 长期记忆（可选）；与 `long_term_memory_scope_id` 配对使用。
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    /// 记忆作用域（如 Web `conversation_id` 或 CLI `cli`）。
    pub long_term_memory_scope_id: Option<String>,
    /// 单轮 `run_agent_turn` 内 `read_file` 结果缓存；`None` 时由 `run_agent_turn` 按配置创建或关闭。
    pub read_file_turn_cache: Option<std::sync::Arc<ReadFileTurnCache>>,
    /// 多角色工作台：本回合允许的工具名；`None` 表示不额外限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// Web `/chat*`：与 **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的结构化日志根 span；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<observability::TracingChatTurn>>,
    /// Web：HTTP 审计（客户端 IP、共享 Bearer 指纹）；CLI/定时任务等为 `None`。
    pub request_audit: Option<Arc<crate::web::audit::WebRequestAudit>>,
    /// 进程内显式句柄：工作区变更集注册表、工具统计等（`bench` 等无 `AppState` 时用 [`crate::process_handles::ProcessHandles::default_arc_process_handles`]）。
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
}

/// 构造 [`RunAgentTurnParams::web_chat_stream`] 所需的参数包（避免长形参列表）。
pub struct WebChatStreamBuildArgs<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub per_flight: std::sync::Arc<chat_job_queue::PerTurnFlight>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    pub temperature_override: Option<f32>,
    pub model_override: Option<String>,
    pub use_executor_model: bool,
    pub executor_model_override: Option<String>,
    pub executor_api_base: Option<String>,
    pub executor_api_key: Option<String>,
    pub seed_override: types::LlmSeedOverride,
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub job_id: u64,
    pub conversation_id: &'a str,
    pub out: &'a mpsc::Sender<String>,
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    pub request_audit: Arc<crate::web::audit::WebRequestAudit>,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
}

/// 构造 [`RunAgentTurnParams::web_chat_json`] 所需的参数包。
pub struct WebChatJsonBuildArgs<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub per_flight: std::sync::Arc<chat_job_queue::PerTurnFlight>,
    pub temperature_override: Option<f32>,
    pub model_override: Option<String>,
    pub use_executor_model: bool,
    pub executor_model_override: Option<String>,
    pub executor_api_base: Option<String>,
    pub executor_api_key: Option<String>,
    pub seed_override: types::LlmSeedOverride,
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub job_id: u64,
    pub conversation_id: &'a str,
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    pub request_audit: Arc<crate::web::audit::WebRequestAudit>,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
}
pub struct CliTerminalChatBuildArgs<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub no_stream: bool,
    /// 为 `true` 时不向 stdout 渲染助手流式/非流式输出（全屏 TUI alternate screen）。
    pub suppress_stdout_render: bool,
    /// 与 **`suppress_stdout_render`** 配套：流式正文写入供 TUI 中区刷新。
    pub tui_llm_stream_scratch: Option<runtime::tui::TuiLlmStreamScratchArc>,
    /// 与 [`AgentTurnTransport::tool_running_hook`] 一致；REPL 等为 `None`。
    pub tool_running_hook: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    /// `present_clarification_questionnaire` 成功时通知 TUI 展示问卷；其它入口 `None`。
    pub clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    pub process_handles: Arc<crate::process_handles::ProcessHandles>,
    /// TUI：SSE 控制面镜像（与 Web `SsePayload` 对齐）；`repl` / `chat` 为 `None`。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

/// `web_chat_stream` / `web_chat_json` 共用的字段装配（单参数传入以满足形参棘轮）。
struct WebChatJobCommonParts<'a> {
    shared: RunAgentTurnSharedInputs<'a>,
    messages: &'a mut Vec<types::Message>,
    effective_working_dir: &'a std::path::Path,
    workspace_is_set: bool,
    transport: AgentTurnTransport<'a>,
    llm: AgentTurnLlmOverrides,
    long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    conversation_id: &'a str,
    turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    tracing_chat_turn: Arc<observability::TracingChatTurn>,
    request_audit: Arc<crate::web::audit::WebRequestAudit>,
    process_handles: Arc<crate::process_handles::ProcessHandles>,
}

impl<'a> RunAgentTurnParams<'a> {
    fn from_web_job_common(parts: WebChatJobCommonParts<'a>) -> Self {
        let WebChatJobCommonParts {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport,
            llm,
            long_term_memory,
            conversation_id,
            turn_allowed_tool_names,
            tracing_chat_turn,
            request_audit,
            process_handles,
        } = parts;
        Self {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport,
            llm,
            long_term_memory,
            long_term_memory_scope_id: Some(conversation_id.to_string()),
            read_file_turn_cache: None,
            turn_allowed_tool_names,
            tracing_chat_turn: Some(tracing_chat_turn),
            request_audit: Some(request_audit),
            process_handles,
        }
    }

    /// Web `/chat/stream`：SSE 输出、可选工具审批、可取消。
    pub fn web_chat_stream(args: WebChatStreamBuildArgs<'a>) -> Self {
        let WebChatStreamBuildArgs {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            cancel,
            per_flight,
            web_tool_ctx,
            temperature_override,
            model_override,
            use_executor_model,
            executor_model_override,
            executor_api_base,
            executor_api_key,
            seed_override,
            long_term_memory,
            job_id,
            conversation_id,
            out,
            turn_allowed_tool_names,
            request_audit,
            process_handles,
        } = args;
        Self::from_web_job_common(WebChatJobCommonParts {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport: AgentTurnTransport {
                out: Some(out),
                render_to_terminal: false,
                no_stream: false,
                cancel: Some(cancel),
                per_flight: Some(per_flight),
                web_tool_ctx,
                cli_tool_ctx: None,
                plain_terminal_stream: false,
                tui_llm_stream_scratch: None,
                tool_running_hook: None,
                clarification_questionnaire_hook: None,
                sse_control_mirror: None,
                llm_backend: None,
            },
            llm: AgentTurnLlmOverrides {
                temperature_override,
                model_override,
                use_executor_model,
                executor_model_override,
                executor_api_base,
                executor_api_key,
                seed_override,
            },
            long_term_memory,
            conversation_id,
            turn_allowed_tool_names,
            tracing_chat_turn: observability::TracingChatTurn::new(job_id, conversation_id),
            request_audit,
            process_handles,
        })
    }

    /// Web `POST /chat`（JSON）：无 SSE，终端渲染管线用于分步通知等。
    pub fn web_chat_json(args: WebChatJsonBuildArgs<'a>) -> Self {
        let WebChatJsonBuildArgs {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            per_flight,
            temperature_override,
            model_override,
            use_executor_model,
            executor_model_override,
            executor_api_base,
            executor_api_key,
            seed_override,
            long_term_memory,
            job_id,
            conversation_id,
            turn_allowed_tool_names,
            request_audit,
            process_handles,
        } = args;
        Self::from_web_job_common(WebChatJobCommonParts {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport: AgentTurnTransport {
                out: None,
                render_to_terminal: true,
                no_stream: false,
                cancel: None,
                per_flight: Some(per_flight),
                web_tool_ctx: None,
                cli_tool_ctx: None,
                plain_terminal_stream: false,
                tui_llm_stream_scratch: None,
                tool_running_hook: None,
                clarification_questionnaire_hook: None,
                sse_control_mirror: None,
                llm_backend: None,
            },
            llm: AgentTurnLlmOverrides {
                temperature_override,
                model_override,
                use_executor_model,
                executor_model_override,
                executor_api_base,
                executor_api_key,
                seed_override,
            },
            long_term_memory,
            conversation_id,
            turn_allowed_tool_names,
            tracing_chat_turn: observability::TracingChatTurn::new(job_id, conversation_id),
            request_audit,
            process_handles,
        })
    }

    /// `chat` 子命令等：本机终端、纯文本流式、可选 `run_command` 交互。
    pub fn cli_terminal_chat(args: CliTerminalChatBuildArgs<'a>) -> Self {
        let CliTerminalChatBuildArgs {
            shared,
            messages,
            effective_working_dir,
            no_stream,
            suppress_stdout_render,
            tui_llm_stream_scratch,
            tool_running_hook,
            clarification_questionnaire_hook,
            cli_tool_ctx,
            long_term_memory,
            long_term_memory_scope_id,
            turn_allowed_tool_names,
            process_handles,
            sse_control_mirror,
        } = args;
        let echo_stdout = !suppress_stdout_render;
        Self {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set: true,
            transport: AgentTurnTransport {
                out: None,
                render_to_terminal: echo_stdout,
                no_stream,
                cancel: None,
                per_flight: None,
                web_tool_ctx: None,
                cli_tool_ctx,
                plain_terminal_stream: echo_stdout,
                tui_llm_stream_scratch,
                tool_running_hook,
                clarification_questionnaire_hook,
                sse_control_mirror,
                llm_backend: None,
            },
            llm: AgentTurnLlmOverrides {
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: types::LlmSeedOverride::default(),
            },
            long_term_memory,
            long_term_memory_scope_id,
            read_file_turn_cache: None,
            turn_allowed_tool_names,
            tracing_chat_turn: None,
            request_audit: None,
            process_handles,
        }
    }

    /// `bench` 批量任务：无终端渲染、非流式、可超时取消。
    pub fn benchmark_batch(
        client: &'a reqwest::Client,
        api_key: &'a str,
        cfg: &'a Arc<config::AgentConfig>,
        tools: &'a [crate::types::Tool],
        messages: &'a mut Vec<types::Message>,
        effective_working_dir: &'a std::path::Path,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            shared: RunAgentTurnSharedInputs {
                client,
                api_key,
                cfg,
                tools,
            },
            messages,
            effective_working_dir,
            workspace_is_set: true,
            transport: AgentTurnTransport {
                out: None,
                render_to_terminal: false,
                no_stream: true,
                cancel: Some(cancel),
                per_flight: None,
                web_tool_ctx: None,
                cli_tool_ctx: None,
                plain_terminal_stream: false,
                tui_llm_stream_scratch: None,
                tool_running_hook: None,
                clarification_questionnaire_hook: None,
                sse_control_mirror: None,
                llm_backend: None,
            },
            llm: AgentTurnLlmOverrides {
                temperature_override: None,
                model_override: None,
                use_executor_model: false,
                executor_model_override: None,
                executor_api_base: None,
                executor_api_key: None,
                seed_override: types::LlmSeedOverride::default(),
            },
            long_term_memory: None,
            long_term_memory_scope_id: None,
            read_file_turn_cache: None,
            turn_allowed_tool_names: None,
            tracing_chat_turn: None,
            request_audit: None,
            process_handles: crate::process_handles::ProcessHandles::default_arc_process_handles(),
        }
    }
}

fn resolved_turn_llm_backend<'a>(
    llm_backend: Option<&'a (dyn llm::ChatCompletionsBackend + 'static)>,
) -> &'a (dyn llm::ChatCompletionsBackend + 'static) {
    match llm_backend {
        Some(b) => b,
        None => llm::default_chat_completions_backend(),
    }
}

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// `cfg` 建议使用 [`Arc`] 共享（与进程内 Web 服务状态一致），以便工具在 `spawn_blocking` 路径中复用同一份配置而不反复深拷贝。
/// 若提供 transport.out，则流式 content 会通过 out 发送（供 SSE 等使用）；`transport.no_stream` 为 true 时 API 使用 `stream: false`，
/// 有正文则通过 `out` 一次性下发整段。
/// 若 `transport.plain_terminal_stream` 为 `true`（仅 **`runtime::cli`** 应传入）：`transport.render_to_terminal` 且 `transport.out` 为 `None` 时，助手正文以**纯文本**流式（或 `--no-stream` 时整段）写入 stdout，不经 `markdown_to_ansi`。
/// 若 `transport.plain_terminal_stream` 为 `false` 且 `transport.render_to_terminal` 为 `true`：仍在整段到达后用 `markdown_to_ansi` 渲染（用于服务端 jobs 等 **`out.is_none()`** 场景，避免与 CLI 混淆）。
/// 当 `transport.out` 为 `None` 且 `transport.render_to_terminal` 为 `true` 时，分阶段规划通知、分步注入 user 与各工具结果另经 `runtime::terminal_cli_transcript` 写入 stdout；通知与注入正文经 `user_message_for_chat_display`（分步长句可压缩）；`transport.plain_terminal_stream` 为 `true` 时助手正文为上游原始增量/拼接，为 `false` 时经 `assistant_markdown_source_for_display` 管线再渲染。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
/// `transport.cancel` 为 `Some` 时，各轮请求会在流式读与重试间隔中轮询其标志；置位后尽快结束并返回 `Ok`（或 `Err`：[`agent::agent_turn::RunAgentTurnError`] 中含取消 / 限流 / SSE 早停等，用户可见串与常量 [`crate::types::LLM_CANCELLED_ERROR`] 对齐），供协作取消等场景使用。
/// 分阶段规划（`staged_plan_execution` / `logical_dual_agent`）下若规划轮未解析出合法 `agent_reply_plan` v1：**不再**整轮失败退出：保留规划轮助手正文并**降级**为与关闭分阶段规划时相同的常规 `run_agent_outer_loop`（含工具）。规划轮会先丢弃 API 返回的原生 `tool_calls`，再从正文 DeepSeek DSML 物化并视情况执行工具，避免网关误报 `tool_calls` 时 CLI 静默无动作。
/// `transport.per_flight` 仅 Web 队列任务传入，用于 `GET /status` 的 `per_active_jobs` 镜像；CLI 传 `None`。
/// 自定义 `ChatCompletionsBackend` 见 [`AgentTurnTransport::llm_backend`]。
pub async fn run_agent_turn<'a>(
    p: RunAgentTurnParams<'a>,
) -> Result<(), crate::agent::agent_turn::RunAgentTurnError> {
    let RunAgentTurnParams {
        shared,
        messages,
        effective_working_dir,
        workspace_is_set,
        transport,
        llm,
        long_term_memory,
        long_term_memory_scope_id,
        read_file_turn_cache,
        turn_allowed_tool_names,
        tracing_chat_turn,
        request_audit,
        process_handles,
    } = p;
    let RunAgentTurnSharedInputs {
        client,
        api_key,
        cfg,
        tools,
    } = shared;
    let AgentTurnTransport {
        out,
        render_to_terminal,
        no_stream,
        cancel,
        per_flight,
        web_tool_ctx,
        cli_tool_ctx,
        plain_terminal_stream,
        tui_llm_stream_scratch,
        tool_running_hook,
        clarification_questionnaire_hook,
        sse_control_mirror,
        llm_backend,
    } = transport;
    let AgentTurnLlmOverrides {
        temperature_override,
        model_override,
        use_executor_model,
        executor_model_override,
        executor_api_base,
        executor_api_key,
        seed_override,
    } = llm;
    let turn_dump_scope_id = long_term_memory_scope_id.clone();
    let turn_dump_model_override = model_override.clone();
    let turn_dump_executor_model_override = executor_model_override.clone();
    let llm_backend = resolved_turn_llm_backend(llm_backend);

    let read_file_turn_cache =
        crate::agent_turn_prep::resolve_read_file_turn_cache_for_turn(cfg, read_file_turn_cache);

    let workspace_changelist = crate::agent_turn_prep::workspace_changelist_for_turn(
        cfg.as_ref(),
        process_handles.as_ref(),
        long_term_memory_scope_id.as_deref(),
    );

    let crate::agent_turn_prep::ToolsForTurnPrepared {
        tools_for_turn,
        mcp_session,
    } = crate::agent_turn_prep::prepare_tools_for_turn(
        cfg,
        tools,
        effective_working_dir,
        turn_allowed_tool_names.as_ref().map(|a| a.as_ref()),
    )
    .await;

    let request_chrome_trace = crate::request_chrome_trace::request_trace_dir_from_env()
        .map(|_| std::sync::Arc::new(crate::request_chrome_trace::RequestTurnTrace::new()));
    let wall_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    crate::turn_replay_dump::set_turn_replay_event_context(
        wall_ms,
        turn_dump_scope_id.as_deref(),
        tracing_chat_turn.as_ref().map(|t| t.job_id),
    );
    crate::turn_replay_dump::append_latest_user_input_event_if_configured(messages);
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "turn_started",
        "run_agent_turn",
        Some(&serde_json::json!({
            "text": format!("wall_start_ms={wall_ms}"),
            "phase": "turn"
        })),
    );

    let mut loop_params = agent::agent_turn::RunLoopParams {
        ctx: agent::agent_turn::RunLoopCtx {
            core: agent::agent_turn::RunLoopCore {
                llm_backend,
                client,
                api_key,
                cfg,
                tools_defs: tools_for_turn.as_slice(),
                effective_working_dir,
                workspace_is_set,
            },
            io: agent::agent_turn::RunLoopIo {
                out,
                no_stream,
                cancel: cancel.as_deref(),
                render_to_terminal,
                plain_terminal_stream,
                tui_llm_stream_scratch,
                tool_running_hook,
                clarification_questionnaire_hook,
                sse_control_mirror,
            },
            attach: agent::agent_turn::RunLoopAttach {
                web_tool_ctx,
                cli_tool_ctx,
                per_flight,
                long_term_memory,
                long_term_memory_scope_id,
                mcp_session,
                read_file_turn_cache,
                workspace_changelist,
                staged_plan_optimizer_round: cfg.staged_planning.staged_plan_optimizer_round,
                staged_plan_optimizer_requires_parallel_tools: cfg
                    .staged_planning
                    .staged_plan_optimizer_requires_parallel_tools,
                staged_plan_ensemble_count: cfg.staged_planning.staged_plan_ensemble_count,
                staged_plan_skip_ensemble_on_casual_prompt: cfg
                    .staged_planning
                    .staged_plan_skip_ensemble_on_casual_prompt,
                turn_allowed_tool_names: turn_allowed_tool_names.clone(),
            },
            obs: agent::agent_turn::RunLoopObs {
                request_chrome_trace: request_chrome_trace.clone(),
                tracing_chat_turn: tracing_chat_turn.clone(),
                request_audit: request_audit.clone(),
                process_handles: Arc::clone(&process_handles),
            },
        },
        turn: agent::agent_turn::RunLoopTurnState {
            messages_buf: messages,
            messages_revision: 0,
            sub_phase: crate::agent::agent_turn::AgentTurnSubPhase::Planner,
            turn_planner_hints: crate::agent::agent_turn::TurnPlannerHints::default(),
            temperature_override,
            model_override,
            use_executor_model,
            executor_model_override,
            executor_api_base,
            executor_api_key,
            seed_override,
        },
    };

    let trace_span = loop_params
        .ctx
        .obs
        .tracing_chat_turn
        .as_ref()
        .map(|t| t.span.clone());
    let run_common = agent::agent_turn::run_agent_turn_common(&mut loop_params);
    let res = match (trace_span, request_chrome_trace) {
        (Some(span), Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common.instrument(span))
                .await
        }
        (Some(span), None) => run_common.instrument(span).await,
        (None, Some(t)) => {
            crate::request_chrome_trace::with_turn_trace(t, wall_ms, run_common).await
        }
        (None, None) => run_common.await,
    };
    crate::turn_replay_dump::write_turn_replay_dump_if_configured(
        crate::turn_replay_dump::TurnReplayDumpParams {
            wall_ms,
            long_term_memory_scope_id: turn_dump_scope_id.as_deref(),
            tracing_job_id: tracing_chat_turn.as_ref().map(|t| t.job_id),
            result: &res,
            messages: loop_params.turn.messages(),
            tools: tools_for_turn.as_slice(),
            cfg: loop_params.ctx.core.cfg,
            no_stream,
            render_to_terminal,
            plain_terminal_stream,
            effective_working_dir,
            workspace_is_set,
            temperature_override,
            model_override: turn_dump_model_override,
            use_executor_model,
            executor_model_override: turn_dump_executor_model_override,
            seed_override,
        },
    );
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "turn_finished",
        "run_agent_turn",
        Some(&serde_json::json!({
            "text": format!("wall_start_ms={wall_ms}, ok={}", res.is_ok()),
            "phase": "turn"
        })),
    );
    crate::turn_replay_dump::clear_turn_replay_event_context();
    res
}

pub(crate) use conversation_store::SaveConversationOutcome;
pub(crate) use web::AppState;
pub(crate) use web::conversation_conflict_sse_line;

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    cli_run::run().await
}

pub use config::{
    AgentConfig, ExposeSecret, LlmHttpAuthMode, PlannerExecutorMode, SharedAgentConfig,
    load_config, load_config_for_cli,
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
pub use types::{
    ChatRequest, FunctionCall, LlmSeedOverride, Message, ToolCall, message_content_as_str,
};

pub use runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT,
    EXIT_TOOL_REPLAY_MISMATCH, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
    classify_model_error_message,
};

#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
