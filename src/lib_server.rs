pub(crate) mod cmd_mate;
/// `crabmatectl` 二进制的实现（同 crate 构建，故 ctl 与 serve 的版本不可能漂移）。
/// `doc(hidden)`：不属公开 API 承诺面，仅供仓内 bin 使用。
#[cfg(unix)]
#[doc(hidden)]
pub mod cm_ctl;
#[doc(hidden)]
pub mod cm_agent;
#[doc(hidden)]
pub mod cm_llm;
pub(crate) mod cm_runtime;
/// 内建工具名等标识符常量的单一真源：零依赖，故列在 `cm_config` 之前。
pub(crate) mod cm_names;
#[doc(hidden)]
pub mod cm_config;
pub(crate) mod cm_tools;
pub(crate) mod cm_memory;
#[doc(hidden)]
pub mod cm_workflow;
pub(crate) mod cm_approval;
#[cfg(feature = "web")]
pub(crate) mod cm_web_host;
pub(crate) mod mcp;
// S4-P1c 桶门面（见 #1300 §1.4）：当前只做转发，P2 才把成员文件搬进来。
pub(crate) mod context;
pub(crate) mod jobs;
pub(crate) mod obs;
pub(crate) mod tool_host;
pub(crate) mod cm_benchmark;

pub mod agent;
/// Docker 沙盒内 `tool-runner-internal` 入口；二进制与 `main` 经此路径调用。
/// Docker 沙盒内 `tool-runner-internal` 入口；S4-P2-5 起为**顶层模块**（§4.2 方案 A）。
pub mod tool_sandbox;
/// `cm_internal` 门面：`#[doc(hidden)]`。稳定对外符号见下方显式 `pub use`（如 `build_tools`、`ProcessHandles`）与 [`tool_sandbox`]。
// `cm_internal` 只导出**自身实现**。`tools` / `workspace` / `redact` / `tool_result` /
// `read_file_turn_cache` 等本体在 `cm_tools`，`memory` 在 `cm_memory` —— 经 `cm_internal`
// 转发只会让内部桶看起来比实际更中心（S4-P1a，见 #1300），故按本体直连。
// 不属任何桶的两个顶层模块（§1.4 修正点 2 / 4）：`process_handles` 是进程级句柄集合，
// 被 tool_host / CLI / run_agent_turn 同时持有，塞进任一个桶都会制造反向依赖；
// `text_util` 只有 22 行，凑进 obs 只是假结构。S4-P2-0 起两者已是**本地顶层模块**
// （`src/process_handles.rs` / `src/text_util.rs`），不再是 `cm_internal` 的转发。
mod process_handles;
mod text_util;
// 其余短名改由四个桶门面接管（S4-P1c）。上层写 `crate::tool_registry` 等不变；
// 变的是这些名字的**定义点**，P2 把文件搬进桶时调用点不再受影响。
pub(crate) use crate::context::{
    agent_errors, agent_role_turn, agent_turn_prep, clarification_questionnaire_body_if_tool_ok,
    context_bootstrap, session_mode_turn, user_message_file_refs,
};
pub(crate) use crate::obs::{health, observability, request_chrome_trace};
pub(crate) use crate::tool_host::{
    memory_tool_hosts, readonly_tool_ttl_cache, tool_call_explain, tool_registry, tool_stats,
};
pub(crate) use crate::cm_memory::memory;
pub(crate) use crate::cm_tools::{
    clarification_questionnaire, github_token, read_file_turn_cache, redact, text_encoding,
    tool_result, tools, workspace,
};
/// SSE 控制面协议与运行时（原 `crate::cm_internal::sse`，已迁至 [`crate::cm_sse_protocol`]）。
#[cfg(feature = "web")]
mod chat_job_queue;
mod cli_run;
mod cm_cli;
/// 集成测试 / `crabmate e2e` 场景入口；**不**承诺 semver。
#[doc(hidden)]
pub mod e2e_scenario;
pub use crate::cm_config as config;
pub use crate::cm_sse_protocol::sse;
pub use crate::cm_types as types;
/// `conversation_id` 持久化（可选 SQLite）。模块本体已在 `src/lib.rs` 按 `sqlite` 门控，
/// 这里只把结果类型留在 crate 内部可见。
pub use crate::cm_llm::http_client;
pub mod llm;
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
/// 测试用 Web 服务器启动器（feature="web"）；集成测试通过 `test_serve::start_test_serve` 启动。**不**承诺 semver。
#[cfg(feature = "web")]
#[doc(hidden)]
pub mod test_serve;
mod turn_replay_dump;
mod turn_runner;
pub use crate::cm_agent::text_sanitize;
mod user_data;
#[cfg(feature = "web")]
mod web;
/// `WorkflowConfig` 从 `AgentConfig` 派生的唯一实现（#1383）：工具面与运维 CLI 共用。
mod workflow_config_bridge;

pub use per_turn_flight::PerTurnFlight;
pub use request_audit::WebRequestAudit;

pub use config::cli::{
    ParsedCliArgs, SaveSessionFormat, ToolReplayCli, WebBearerCli, normalize_help_argv,
    parse_args, parse_args_from_argv, root_clap_command_for_man_page,
};
pub use read_file_turn_cache::{ReadFileTurnCache, ReadFileTurnCacheHandle, new_turn_cache_handle};
pub use run_agent_turn::run_agent_turn;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
// **S6-d（#1298）：端口 trait 与它们的签名闭包进承诺面。**
//
// 承诺的理由是「外部有理由自己实现」，不是「名字好听」：`ToolExecutionHost` /
// `ToolDispatch` / `ToolOutcomeRecorder` 的签名里就是 crate 内部类型，故**不**上根
// （见 `docs/design/sdk_facade.md` §7.2）；下面这四组都能被下游完整实现。
//
// 符号级 re-export（而非把 `cm_approval` / `turn_runner` / `conversation_store`
// 整模块 pub）：源模块保持 `pub(crate)`，只有承诺的名字出去——否则内部项会随模块
// 可见性一起变成事实上的公开面，而清单门禁看不到它们。
pub use turn_runner::{DefaultTurnEngine, TurnEngine, default_turn_engine};
pub use cm_agent::permission::PolicyEngine;
pub use cm_approval::{ApprovalBroker, WebApprovalSink};
pub use conversation_store::{ArchiveReason, ArchiveRow, BranchTruncateOutcome, SaveConversationOutcome};
pub use cm_session_store::{ConversationRetention, ConversationTurnSeed, SessionStore};
pub use crate::agent::agent_turn::host::errors::{
    AgentTurnSubPhase, RunAgentTurnError, TurnAbortReason,
};
/// 端口以 `#[async_trait]` 声明。不 re-export 宏，外部实现就只能自己猜版本；
/// 承诺 trait 却不承诺它要求的宏，等于把「可实现」变成一句口号。
pub use async_trait::async_trait;
// `run_agent_turn` 是 crate 根的公开入口，其返回类型因此必须公开可达（S2-4，见 #1285）。
pub use turn_runner::{TurnOutcome, TurnStats};

/// 回合传输与端点表现（SSE、取消、审批上下文等），与模型采样/路由覆盖解耦。
pub struct AgentTurnTransport<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub no_stream: bool,
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub per_flight: Option<std::sync::Arc<PerTurnFlight>>,
    pub web_tool_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
    /// 无 `/chat/stream` 通道时镜像 SSE 控制面（与 Web [`crate::sse::SsePayload`] 同形），供 TUI 等终端界面展示。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 可选：自定义 [`llm::ChatCompletionsBackend`]；`None` 时使用 OpenAI 兼容 HTTP（与历史行为一致）。
    pub llm_backend: Option<&'a (dyn llm::ChatCompletionsBackend + 'static)>,
    /// 可选：per-step trace sink（bench/e2e 注入；`None` 时零开销）。
    /// 持有 [`crate::llm::TraceSink`] 以便在 LLM 请求/响应、工具调用前后 emit [`crate::llm::TraceEvent`]。
    pub trace_sink: Option<std::sync::Arc<dyn crate::llm::TraceSink>>,
}

/// 本回合对 `chat/completions` 的采样与模型路由覆盖（相对 [`config::AgentConfig`]）。
pub struct AgentTurnLlmOverrides {
    /// 覆盖本回合 `chat/completions` 的 **`temperature`**（`None` 则用 `config::AgentConfig::temperature`）。
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
    /// 权限 `min` 链的四档输入（角色上限 / 会话模式 / 请求等级 / 会话持久意图）。
    ///
    /// 收成结构体而不是四个平铺字段：`run_agent_turn` 的 nloc 已贴上限，S6 还要往里加轴。
    pub permission: crate::cm_agent::permission::TurnPermissionStages,
}

/// 可观测与进程句柄（入口袋；与环内 `RunLoopObs` 对应，勿与之混淆）。
pub struct RunAgentTurnObs {
    /// Web `/chat*`：与 **`x-stream-job-id`** / SSE **`sse_capabilities.job_id`** 对齐的结构化日志根 span；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<observability::TracingChatTurn>>,
    /// Web：HTTP 审计（客户端 IP、共享 Bearer 指纹）；CLI/定时任务等为 `None`。
    pub request_audit: Option<Arc<WebRequestAudit>>,
    /// 进程内显式句柄：工作区变更集注册表、工具统计等（`bench` 等无 `AppState` 时用 [`crate::process_handles::TurnProcessHandles::default_arc`]；完整袋见 [`ProcessHandles`]）。
    pub process_handles: Arc<crate::process_handles::TurnProcessHandles>,
    /// 后台任务注册表（`run_command` 的 `async=true`）；`None` 时返回未启用（默认关闭路径）。
    pub tool_job_registry: Option<Arc<crate::jobs::tool_jobs::ToolJobRegistry>>,
    /// `#1455` C 步：把「本回合因预算暂停」这一事实带回调用方。
    ///
    /// 用 `Arc<Mutex<_>>` 而不是让 `run_agent_turn` 返回它，是因为**预算拒绝走 `Err`**、
    /// `TurnOutcome` 只在成功路径构造；而扩 `RunAgentTurnError` 会波及所有 `match` 点。
    /// 调用方创建、传入、回合结束后读——回合已结束时该句柄仍然有效。
    pub paused_reason_sink:
        Option<std::sync::Arc<std::sync::Mutex<Option<crate::cm_agent::task_state::TaskPauseReason>>>>,
}

/// Web/CLI/基准测试共用的 `run_agent_turn` 入参（避免长参数列表）。
///
/// 顶层分组（turn_host **P3d**）：`shared` / `session` / `transport` / `llm` / `attach` / `obs`（字段不删，仅嵌套）。
pub struct TurnRequest<'a> {
    pub shared: RunAgentTurnSharedInputs<'a>,
    pub session: RunAgentTurnSession<'a>,
    pub transport: AgentTurnTransport<'a>,
    pub llm: AgentTurnLlmOverrides,
    pub attach: RunAgentTurnAttach,
    pub obs: RunAgentTurnObs,
}

/// 旧名兼容别名（S2 正名，见 #1285）：新代码请用 [`TurnRequest`]。
///
/// `TurnRequest` 经 `include!("lib_server.rs")` 落在 crate 根，属公开 API，
/// 故保留一版别名供外部调用方迁移；本仓内已全部改用新名（`-D warnings` 下用旧名会失败）。
#[deprecated(note = "改名为 TurnRequest（S2，见 #1285）")]
pub type RunAgentTurnParams<'a> = TurnRequest<'a>;

#[cfg(feature = "web")]
#[doc(hidden)]
pub enum WebTurnEndpoint<'a> {
    /// `/chat/stream`：SSE 输出、可取消、可走工具审批、可回传暂停原因。
    Stream {
        out: &'a mpsc::Sender<String>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        web_tool_ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
        /// `#1455` C 步：预算暂停的回传口（`None` = 调用方不关心）。
        ///
        /// 放在**变体**里而不是 `WebTurnInputs`：自动续跑对非流式没有语义，让 JSON 端点
        /// 在类型上就拿不到它，而不是靠在字段 doc 里写「恒 `None`」。
        paused_reason_sink: Option<
            std::sync::Arc<std::sync::Mutex<Option<crate::cm_agent::task_state::TaskPauseReason>>>,
        >,
    },
    /// `POST /chat`：无 SSE 通道、无取消、无工具审批、无暂停回传。
    Json,
}

impl<'a> WebTurnEndpoint<'a> {
    /// 端点差异**只**落在这里：JSON 侧 `out` / `cancel` / `web_tool_ctx` /
    /// `paused_reason_sink` 恒 `None`。
    ///
    /// 返回两部分而非只返回 transport：暂停回传口落在 `obs` 而不是 `transport`，但它和另外
    /// 三项一样是端点专属的，收在同一个 `match` 里才不会漏。
    fn into_parts(
        self,
        per_flight: std::sync::Arc<PerTurnFlight>,
    ) -> (
        AgentTurnTransport<'a>,
        Option<std::sync::Arc<std::sync::Mutex<Option<crate::cm_agent::task_state::TaskPauseReason>>>>,
    ) {
        let (out, cancel, web_tool_ctx, paused_reason_sink) = match self {
            Self::Stream {
                out,
                cancel,
                web_tool_ctx,
                paused_reason_sink,
            } => (Some(out), Some(cancel), web_tool_ctx, paused_reason_sink),
            Self::Json => (None, None, None, None),
        };
        (
            AgentTurnTransport {
                out,
                no_stream: false,
                cancel,
                per_flight: Some(per_flight),
                web_tool_ctx,
                sse_control_mirror: None,
                llm_backend: None,
                trace_sink: None,
            },
            paused_reason_sink,
        )
    }
}

/// Web 端点专属的回合输入；通用输入的 `shared` / `session` / `llm` / `per_flight`
/// 由 [`TurnRequest::web`] 的参数原样承接。
///
/// 取代 `WebChatStreamBuildArgs`（**25** 字段）/ `WebChatJsonBuildArgs`（**22** 字段）两个平铺袋，
/// 以及它们与 `TurnRequest` 之间的 `WebChatJobCommonParts` / `from_web_job_common` 两层转接（#1379）。
/// 这里只留**只有 Web 端点才有**的输入；端点差异（SSE 输出 / 取消 / 审批 / 暂停回传）另由
/// [`WebTurnEndpoint`] 在类型上表达，而不是靠「`Option` 恒 `None`」的口头约定。
///
/// `#[doc(hidden)]` + 不承诺 semver（S6，见 #1297 与 `fixtures/public_api_golden.txt`）：本仓
/// `chat_job_queue` 仍在构造它，故刻意**不**标 `#[deprecated]`——`-D warnings` 会把内部调用点变成编译错误。
#[cfg(feature = "web")]
#[doc(hidden)]
pub struct WebTurnInputs<'a> {
    /// 队列任务号；派生 SSE 日志根 span。
    pub job_id: u64,
    /// 会话 id：长期记忆作用域 + trace span 字段。
    pub conversation_id: &'a str,
    /// 与 HTTP **`x-request-id`** 同值；写入 [`observability::TracingChatTurn`] 供回合内 SSE 错误带回。
    pub request_id: Option<String>,
    pub request_audit: std::sync::Arc<WebRequestAudit>,
    pub process_handles: std::sync::Arc<crate::process_handles::TurnProcessHandles>,
    /// 权限 `min` 链的四档输入；见 [`RunAgentTurnAttach::permission`]。
    pub permission: crate::cm_agent::permission::TurnPermissionStages,
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    /// 多角色工作台：本回合允许的工具名；`None` 表示不额外限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// 后台任务注册表（`run_command` 的 `async=true`）；`None` 时返回未启用。
    pub tool_job_registry: Option<std::sync::Arc<crate::jobs::tool_jobs::ToolJobRegistry>>,
    /// 端点差异（SSE / JSON）；见 [`WebTurnEndpoint`]。
    pub endpoint: WebTurnEndpoint<'a>,
}

impl<'a> TurnRequest<'a> {
    /// Web 端点（`/chat/stream`、`POST /chat`）的回合请求。
    ///
    /// 取代 `web_chat_stream` / `web_chat_json` 与两个 25/22 字段的参数袋（#1379）。三处派生收在这里，
    /// 调用方不再各自复制：记忆作用域取 `conversation_id`、trace span 由
    /// `job_id + conversation_id + request_id` 构造、transport 的 `sse_control_mirror` /
    /// `llm_backend` / `trace_sink` 保持 `None`（需要注入的调用方在拿到结果后再赋值）。
    #[cfg(feature = "web")]
    pub fn web(
        shared: RunAgentTurnSharedInputs<'a>,
        session: RunAgentTurnSession<'a>,
        llm: AgentTurnLlmOverrides,
        // 与 transport 同源：`#1380` 的在册语义是「持有这个 `Arc` 就出现在 `GET /status` 里」，
        // 所以它不进 `WebTurnInputs`。
        per_flight: std::sync::Arc<PerTurnFlight>,
        inputs: WebTurnInputs<'a>,
    ) -> Self {
        let WebTurnInputs {
            job_id,
            conversation_id,
            request_id,
            request_audit,
            process_handles,
            permission,
            long_term_memory,
            turn_allowed_tool_names,
            tool_job_registry,
            endpoint,
        } = inputs;
        let tracing_chat_turn =
            observability::TracingChatTurn::new(job_id, conversation_id, request_id);
        let (transport, paused_reason_sink) = endpoint.into_parts(per_flight);
        Self {
            shared,
            session,
            transport,
            llm,
            attach: RunAgentTurnAttach {
                long_term_memory,
                long_term_memory_scope_id: Some(conversation_id.to_string()),
                read_file_turn_cache: None,
                turn_allowed_tool_names,
                permission,
            },
            obs: RunAgentTurnObs {
                tracing_chat_turn: Some(tracing_chat_turn),
                request_audit: Some(request_audit),
                process_handles,
                tool_job_registry,
                paused_reason_sink,
            },
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
                permission: crate::cm_agent::permission::TurnPermissionStages {
                    role_max: None,
                    session_mode: types::SessionMode::Act,
                    requested: None,
                    persisted: None,
                },
            },
            obs: RunAgentTurnObs {
                tracing_chat_turn: None,
                request_audit: None,
                process_handles: crate::process_handles::TurnProcessHandles::default_arc(),
                tool_job_registry: None,
                // bench 不参与自动续跑：不接暂停信号。
                paused_reason_sink: None,
            },
        }
    }
}

#[cfg(feature = "web")]
pub(crate) use web::AppState;

/// `serve` 装配用的后台任务注册表：按配置建表并挂 TTL 清理定时器（终态过期 → 轮询 410）。
///
/// 放在根包而不是 `cm_cli::serve`：`cm_cli/` 被禁边（`scripts/check-crate-deps.sh`）
/// 不许引用 `crate::cm_internal`，这里是那两处调用的唯一门面。
#[cfg(feature = "web")]
pub(crate) fn build_serve_tool_job_registry(
    cfg: &config::AgentConfig,
) -> std::sync::Arc<crate::jobs::tool_jobs::ToolJobRegistry> {
    let registry = crate::jobs::tool_jobs::registry_from_config(cfg);
    crate::jobs::tool_jobs::spawn_cleanup_task(
        std::sync::Arc::clone(&registry),
        std::time::Duration::from_secs(60),
    );
    registry
}
#[cfg(feature = "web")]
pub(crate) use web::conversation_conflict_sse_line;

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL 等。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    cli_run::run().await
}

/// 已解析 CLI 参数后的入口；`main` 在 `block_on` 时优先调用本函数以减小 future 嵌套深度。
///
/// `#[doc(hidden)]` + 不承诺 semver（S6，见 #1297）：`pub` 是 crate 根 `include!` 布局的
/// 副作用——`src/main.rs` 是独立 crate target，必须经根上路径调用。它不构成 SDK 面。
#[doc(hidden)]
pub async fn run_cli_from_parsed(
    args: config::cli::ParsedCliArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    Box::pin(cli_run::run_cli_from_parsed(args)).await
}

pub use config::{
    AgentConfig, ExposeSecret, LlmHttpAuthMode, SharedAgentConfig, load_config, load_config_for_cli,
};
pub use llm::{
    ChatCompletionsBackend, CompleteChatRetryingParams, E2eMode, OPENAI_COMPAT_BACKEND,
    OpenAiCompatBackend, StreamChatParams, TraceEvent, TraceSink, default_chat_completions_backend,
    shared_static_chat_backend,
};
pub use crate::cm_memory::memory::long_term_memory::LongTermMemoryRuntime;
pub use observability::TracingChatTurn;
pub use tool_registry::{
    ToolDispatchMeta, ToolExecutionClass, WebToolRuntime, all_dispatch_metadata,
    execution_class_for_tool, is_readonly_tool, try_dispatch_meta,
};
pub use tools::dev_tag;
pub use tools::{ToolsBuildOptions, build_tools, build_tools_filtered, build_tools_with_options};
pub use types::{
    ChatRequest, FunctionCall, LlmSeedOverride, Message, ToolCall, message_content_as_str,
};

pub use runtime::cli_exit::{
    CliExitError, EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT,
    EXIT_TOOL_REPLAY_MISMATCH, EXIT_TOOLS_ALL_RUN_COMMAND_DENIED, EXIT_USAGE,
};
pub use agent_errors::classify_model_error_message;

#[cfg(test)]
mod tests;

/// `WebTurnEndpoint` → （`AgentTurnTransport`, 暂停回传口）的映射锚（#1379）。
///
/// 这是那次重构**唯一新增的分支逻辑**：此前两处 worker 各自手写 `None` / `Some`，漏一项不会有
/// 编译错误，只会在流式侧静默丢掉取消位、审批上下文或暂停回传口。两条断言分别钉住两个变体。
#[cfg(all(test, feature = "web"))]
mod web_turn_endpoint_tests {
    use super::{PerTurnFlight, WebTurnEndpoint};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// JSON 端点必须是「静默」的：无输出通道、无取消位、无审批上下文、无暂停回传。
    #[test]
    fn json_endpoint_keeps_transport_silent() {
        let (transport, paused) =
            WebTurnEndpoint::Json.into_parts(Arc::new(PerTurnFlight::default()));
        assert!(transport.out.is_none(), "JSON 端点不应有 SSE 输出通道");
        assert!(transport.cancel.is_none(), "JSON 端点不应有取消位");
        assert!(transport.web_tool_ctx.is_none(), "JSON 端点不应有审批上下文");
        assert!(paused.is_none(), "JSON 端点不应有暂停回传口");
        assert!(
            transport.per_flight.is_some(),
            "per_flight 两侧都必须在册（#1380）"
        );
        assert!(!transport.no_stream, "JSON 端点沿用既有的 no_stream=false");
    }

    /// Stream 端点的三个 transport 项与暂停回传口都必须落到位。
    #[test]
    fn stream_endpoint_carries_sse_cancel_and_approval() {
        let (tx, _rx) = mpsc::channel::<String>(1);
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sink = Arc::new(std::sync::Mutex::new(None));
        let (transport, paused) = WebTurnEndpoint::Stream {
            out: &tx,
            cancel: Arc::clone(&cancel),
            web_tool_ctx: None,
            paused_reason_sink: Some(Arc::clone(&sink)),
        }
        .into_parts(Arc::new(PerTurnFlight::default()));
        assert!(transport.out.is_some(), "Stream 端点必须带 SSE 输出通道");
        assert!(transport.cancel.is_some(), "Stream 端点必须带取消位");
        assert!(transport.per_flight.is_some());
        assert!(!transport.no_stream);
        assert!(paused.is_some(), "Stream 端点必须把暂停回传口带出去");
    }
}
