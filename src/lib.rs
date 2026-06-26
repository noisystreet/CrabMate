//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 **`tracing`** + **`tracing-subscriber`** 处理，**`tracing-log`** 桥接既有 `log::` 调用；`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

pub mod agent;
pub use crabmate_internal::{
    agent_errors, agent_role_turn, agent_turn_prep, cargo_metadata, clarification_questionnaire,
    context_bootstrap, dsml, dynamic_tools, health, health_dep_compat, mcp, memory, observability,
    process_handles, read_file_turn_cache, readonly_tool_ttl_cache, redact, request_chrome_trace,
    sse, text_encoding, text_util, tool_approval, tool_call_explain, tool_registry, tool_result,
    tool_sandbox, tool_stats, tools, user_message_file_refs, web_static_dir, workspace,
};
mod chat_job_queue;
mod cli_run;
pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_llm;
/// Web `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
mod conversation_store;
pub use crabmate_llm::http_client;
mod llm;
/// 元对话门控补充（如「我刚才问了什么」类追问）。
mod meta_dialogue;
pub use process_handles::ProcessHandles;

/// 仅 **`cargo test`**：清空 **`run_command`** 全局限流状态与 **`test_result_cache`** LRU，减轻测试顺序依赖。
#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    crate::tools::reset_process_tool_globals_for_tests();
    crate::turn_replay_dump::reset_turn_replay_globals_for_tests();
}

mod run_agent_turn;
mod runtime;
mod turn_replay_dump;
pub use crabmate_agent::text_sanitize;
pub use crabmate_types;
pub use crabmate_types as types;
mod user_data;
mod web;

pub use config::cli::{
    ChatCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionFormat, ToolReplayCli,
    normalize_legacy_argv, parse_args, parse_args_from_argv, root_clap_command_for_man_page,
};
pub use read_file_turn_cache::{ReadFileTurnCache, ReadFileTurnCacheHandle, new_turn_cache_handle};
pub use run_agent_turn::run_agent_turn;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

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

pub(crate) use conversation_store::SaveConversationOutcome;
pub(crate) use web::AppState;
pub(crate) use web::conversation_conflict_sse_line;

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    cli_run::run().await
}

/// 已解析 CLI 参数后的入口；[`main`](crate::main) 在 `block_on` 时优先调用本函数以减小 future 嵌套深度。
pub async fn run_cli_from_parsed(
    args: config::cli::ParsedCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    Box::pin(cli_run::run_cli_from_parsed(args)).await
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
