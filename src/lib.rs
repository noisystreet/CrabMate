//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 **`tracing`** 处理；**`observability::init_tracing_subscriber`**（`crabmate-internal`）安装 **`tracing-subscriber`** 并用 **`tracing-log`** 桥接既有 `log::` 调用。`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

pub mod agent;
/// Docker 沙盒内 `tool-runner-internal` 入口；二进制与 `main` 经此路径调用。
pub use crabmate_internal::tool_sandbox;
/// `crabmate-internal` 门面：仅本包（及同 crate 集成路径）可见，避免把服务内部模块整包升格为公共 API。
/// 稳定对外符号见下方显式 `pub use`（如 `build_tools`、`ProcessHandles`）与 [`tool_sandbox`]。
pub(crate) use crabmate_internal::{
    agent_errors, agent_role_turn, agent_turn_prep, clarification_questionnaire,
    clarification_questionnaire_body_if_tool_ok, context_bootstrap, github_token, health, mcp,
    memory, memory_tool_hosts, observability, process_handles, read_file_turn_cache,
    readonly_tool_ttl_cache, redact, request_chrome_trace, session_mode_turn, text_encoding,
    text_util, tool_call_explain, tool_registry, tool_result, tool_stats, tools,
    user_message_file_refs, web_static_dir, workspace,
};
/// SSE 控制面协议与运行时（原 `crabmate_internal::sse`，已迁移至 `crabmate-sse-protocol`）。
pub use crabmate_sse_protocol::sse;
#[cfg(feature = "web")]
mod chat_job_queue;
mod cli_run;
pub mod e2e_scenario;
mod env_flags;
pub use crabmate_config;
pub use crabmate_config as config;
pub use crabmate_llm;
/// Web `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
#[cfg(feature = "web")]
mod conversation_store;
pub use crabmate_llm::http_client;
mod llm;
/// 元对话门控补充（如「我刚才问了什么」类追问）。
mod meta_dialogue;
mod per_turn_flight;
pub use process_handles::ProcessHandles;
pub use process_handles::TurnProcessHandles;
mod request_audit;
mod shutdown;

/// 仅 **`cargo test`**：清空 **`run_command`** 全局限流状态与 **`test_result_cache`** LRU，减轻测试顺序依赖。
#[cfg(test)]
pub fn reset_process_tool_globals_for_tests() {
    crate::tools::reset_process_tool_globals_for_tests();
    crate::turn_replay_dump::reset_turn_replay_globals_for_tests();
}

mod run_agent_turn;
mod runtime;
/// 测试用 Web 服务器启动器（feature="web"）；集成测试通过公开的 [`test_serve::start_test_serve`] 快速启动。
#[cfg(feature = "web")]
pub mod test_serve;
mod turn_replay_dump;
mod turn_runner;
pub use crabmate_agent::text_sanitize;
pub use crabmate_types;
pub use crabmate_types as types;
mod user_data;
#[cfg(feature = "web")]
mod web;

pub use per_turn_flight::PerTurnFlight;
pub use request_audit::WebRequestAudit;

pub use config::cli::{
    E2eCliArgs, ExtraCliCommand, ParsedCliArgs, SaveSessionFormat, ToolReplayCli, WebBearerCli,
    normalize_legacy_argv, parse_args, parse_args_from_argv, root_clap_command_for_man_page,
};
pub use read_file_turn_cache::{ReadFileTurnCache, ReadFileTurnCacheHandle, new_turn_cache_handle};
pub use run_agent_turn::run_agent_turn;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
#[allow(unused_imports)] // `DefaultTurnRunner` 供文档与类型探查；装配走 `default_turn_runner`
pub(crate) use turn_runner::{DefaultTurnRunner, TurnRunner, default_turn_runner};

/// 回合传输与端点表现（SSE、取消、审批上下文等），与模型采样/路由覆盖解耦。
pub struct AgentTurnTransport<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub no_stream: bool,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub per_flight: Option<std::sync::Arc<PerTurnFlight>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 无 `/chat/stream` 通道时镜像 SSE 控制面（与 Web [`crate::sse::SsePayload`] 同形），供 TUI 等终端界面展示。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 可选：自定义 [`llm::ChatCompletionsBackend`]；`None` 时使用 OpenAI 兼容 HTTP（与历史行为一致）。
    pub llm_backend: Option<&'a (dyn llm::ChatCompletionsBackend + 'static)>,
    /// 可选：per-step trace sink（bench/e2e 注入；`None` 时零开销）。
    /// 持有 [`crabmate_llm::TraceSink`] 以便在 LLM 请求/响应、工具调用前后 emit [`crabmate_llm::TraceEvent`]。
    pub trace_sink: Option<std::sync::Arc<dyn crabmate_llm::TraceSink>>,
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

/// 会话消息与工作区（入口袋；与环内 `RunLoopCore` 工作区字段对应）。
pub struct RunAgentTurnSession<'a> {
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
}

/// 记忆 / 工具策略附件（入口袋；进环后映射到 `RunLoopAttach` 相关字段）。
pub struct RunAgentTurnAttach {
    /// 长期记忆（可选）；与 `long_term_memory_scope_id` 配对使用。
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    /// 记忆作用域（如 Web `conversation_id` 或 CLI `cli`）。
    pub long_term_memory_scope_id: Option<String>,
    /// 单轮 `run_agent_turn` 内 `read_file` 结果缓存；`None` 时由 `run_agent_turn` 按配置创建或关闭。
    pub read_file_turn_cache: Option<std::sync::Arc<ReadFileTurnCache>>,
    /// 多角色工作台：本回合允许的工具名；`None` 表示不额外限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// 本回合会话工作模式（Ask/Plan/Act）。
    pub session_mode: types::SessionMode,
}

/// 可观测与进程句柄（入口袋；与环内 `RunLoopObs` 对应，勿与之混淆）。
pub struct RunAgentTurnObs {
    /// Web `/chat*`：与 **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的结构化日志根 span；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<observability::TracingChatTurn>>,
    /// Web：HTTP 审计（客户端 IP、共享 Bearer 指纹）；CLI/定时任务等为 `None`。
    pub request_audit: Option<Arc<WebRequestAudit>>,
    /// 进程内显式句柄：工作区变更集注册表、工具统计等（`bench` 等无 `AppState` 时用 [`crate::process_handles::TurnProcessHandles::default_arc`]；完整袋见 [`ProcessHandles`]）。
    pub process_handles: Arc<crate::process_handles::TurnProcessHandles>,
}

/// Web/CLI/基准测试共用的 `run_agent_turn` 入参（避免长参数列表）。
///
/// 顶层分组（turn_host **P3d**）：`shared` / `session` / `transport` / `llm` / `attach` / `obs`（字段不删，仅嵌套）。
pub struct RunAgentTurnParams<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub session: RunAgentTurnSession<'a>,
    pub transport: AgentTurnTransport<'a>,
    pub llm: AgentTurnLlmOverrides,
    pub attach: RunAgentTurnAttach,
    pub obs: RunAgentTurnObs,
}

/// 构造 [`RunAgentTurnParams::web_chat_stream`] 所需的参数包（避免长形参列表）。
#[cfg(feature = "web")]
pub struct WebChatStreamBuildArgs<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub per_flight: std::sync::Arc<PerTurnFlight>,
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
    /// 与 HTTP **`x-request-id`** 同值；写入 [`observability::TracingChatTurn`] 供回合内 SSE 错误带回。
    pub request_id: Option<String>,
    pub out: &'a mpsc::Sender<String>,
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    pub request_audit: Arc<WebRequestAudit>,
    pub process_handles: Arc<crate::process_handles::TurnProcessHandles>,
    pub session_mode: types::SessionMode,
}

/// 构造 [`RunAgentTurnParams::web_chat_json`] 所需的参数包。
#[cfg(feature = "web")]
pub struct WebChatJsonBuildArgs<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub messages: &'a mut Vec<types::Message>,
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub per_flight: std::sync::Arc<PerTurnFlight>,
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
    /// 与 HTTP **`x-request-id`** 同值（JSON 路径当前多为 `None`；与 stream 字段形状对齐）。
    pub request_id: Option<String>,
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    pub request_audit: Arc<WebRequestAudit>,
    pub process_handles: Arc<crate::process_handles::TurnProcessHandles>,
    pub session_mode: types::SessionMode,
}
/// `web_chat_stream` / `web_chat_json` 共用的字段装配（单参数传入以满足形参棘轮）。
#[cfg(feature = "web")]
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
    session_mode: types::SessionMode,
    tracing_chat_turn: Arc<observability::TracingChatTurn>,
    request_audit: Arc<WebRequestAudit>,
    process_handles: Arc<crate::process_handles::TurnProcessHandles>,
}

impl<'a> RunAgentTurnParams<'a> {
    #[cfg(feature = "web")]
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
            session_mode,
            tracing_chat_turn,
            request_audit,
            process_handles,
        } = parts;
        Self {
            shared,
            session: RunAgentTurnSession {
                messages,
                effective_working_dir,
                workspace_is_set,
            },
            transport,
            llm,
            attach: RunAgentTurnAttach {
                long_term_memory,
                long_term_memory_scope_id: Some(conversation_id.to_string()),
                read_file_turn_cache: None,
                turn_allowed_tool_names,
                session_mode,
            },
            obs: RunAgentTurnObs {
                tracing_chat_turn: Some(tracing_chat_turn),
                request_audit: Some(request_audit),
                process_handles,
            },
        }
    }

    /// Web `/chat/stream`：SSE 输出、可选工具审批、可取消。
    #[cfg(feature = "web")]
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
            session_mode,
            request_id,
        } = args;
        Self::from_web_job_common(WebChatJobCommonParts {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport: AgentTurnTransport {
                out: Some(out),
                no_stream: false,
                cancel: Some(cancel),
                per_flight: Some(per_flight),
                web_tool_ctx,
                sse_control_mirror: None,
                llm_backend: None,
                trace_sink: None,
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
            session_mode,
            tracing_chat_turn: observability::TracingChatTurn::new(
                job_id,
                conversation_id,
                request_id,
            ),
            request_audit,
            process_handles,
        })
    }

    /// Web `POST /chat`（JSON）：无 SSE；不向 serve 进程 stdout 回显助手/工具输出。
    #[cfg(feature = "web")]
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
            session_mode,
            request_id,
        } = args;
        Self::from_web_job_common(WebChatJobCommonParts {
            shared,
            messages,
            effective_working_dir,
            workspace_is_set,
            transport: AgentTurnTransport {
                out: None,
                no_stream: false,
                cancel: None,
                per_flight: Some(per_flight),
                web_tool_ctx: None,
                sse_control_mirror: None,
                llm_backend: None,
                trace_sink: None,
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
            session_mode,
            tracing_chat_turn: observability::TracingChatTurn::new(
                job_id,
                conversation_id,
                request_id,
            ),
            request_audit,
            process_handles,
        })
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
            session: RunAgentTurnSession {
                messages,
                effective_working_dir,
                workspace_is_set: true,
            },
            transport: AgentTurnTransport {
                out: None,
                no_stream: true,
                cancel: Some(cancel),
                per_flight: None,
                web_tool_ctx: None,
                sse_control_mirror: None,
                llm_backend: None,
                trace_sink: None,
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
            attach: RunAgentTurnAttach {
                long_term_memory: None,
                long_term_memory_scope_id: None,
                read_file_turn_cache: None,
                turn_allowed_tool_names: None,
                session_mode: types::SessionMode::Act,
            },
            obs: RunAgentTurnObs {
                tracing_chat_turn: None,
                request_audit: None,
                process_handles: crate::process_handles::TurnProcessHandles::default_arc(),
            },
        }
    }
}

#[cfg(feature = "web")]
pub(crate) use conversation_store::SaveConversationOutcome;
#[cfg(feature = "web")]
pub(crate) use web::AppState;
#[cfg(feature = "web")]
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
    StreamChatParams, default_chat_completions_backend, shared_static_chat_backend,
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
