//! Web/CLI 共用：外层循环与分阶段规划的运行期参数。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::workspace_changelist::WorkspaceChangelist;

use tokio::sync::mpsc;

use super::errors::AgentTurnSubPhase;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::long_term_memory::LongTermMemoryRuntime;
use crate::tool_registry;
use crate::types::{LlmSeedOverride, Message};

/// Web/CLI 共用：外层循环与分阶段规划注入共用的一套运行期参数。
pub(crate) struct RunLoopParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub render_to_terminal: bool,
    /// 见 [`crate::llm::api::stream_chat`] 的 `plain_terminal_stream`；仅 CLI 入口为 `true`。
    pub plain_terminal_stream: bool,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 与 [`WebExecuteCtx::cli_tool_ctx`] 相同；Web 队列传 `None`。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub per_flight: Option<Arc<crate::chat_job_queue::PerTurnFlight>>,
    /// `None` 时使用 `cfg.temperature`。
    pub temperature_override: Option<f32>,
    /// 覆盖本回合的 `model`（`None` 时使用 `cfg.model`）
    pub model_override: Option<String>,
    pub seed_override: LlmSeedOverride,
    /// 长期记忆运行时（Web 或 CLI）；`None` 时不注入/不索引。
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    /// `conversation_id` 或 CLI 固定 `cli`；`None` 时不按会话隔离（跳过记忆）。
    pub long_term_memory_scope_id: Option<String>,
    /// MCP stdio 会话；`None` 时不处理 `mcp__*` 工具名。
    pub mcp_session: Option<Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
    /// 单轮内 `read_file` 磁盘缓存；`None` 且配置启用时由 `run_agent_turn` 创建。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    /// 本会话工作区变更集；`None` 时不记录/不注入（见 `session_workspace_changelist_*` 配置）。
    pub workspace_changelist: Option<Arc<WorkspaceChangelist>>,
    /// 分阶段规划首轮成功后，是否再跑一轮无工具「步骤优化」（合并无依赖只读探查步等）。默认 true。
    pub staged_plan_optimizer_round: bool,
    /// 无「可同轮并行批处理」内建工具时是否跳过优化轮。见 `AgentConfig::staged_plan_optimizer_requires_parallel_tools`。
    pub staged_plan_optimizer_requires_parallel_tools: bool,
    /// 逻辑多规划员：首轮后的独立规划份数上限（1=关闭）。见 `AgentConfig::staged_plan_ensemble_count`。
    pub staged_plan_ensemble_count: u8,
    /// 寒暄/极短用户输入时是否跳过 ensemble。见 `AgentConfig::staged_plan_skip_ensemble_on_casual_prompt`。
    pub staged_plan_skip_ensemble_on_casual_prompt: bool,
    /// 整请求 Chrome trace（`CRABMATE_REQUEST_CHROME_TRACE_DIR`）；`None` 关闭。
    pub request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    /// 分阶段规划当前步的「子代理」工具约束；`None` 表示不限制（常规循环）。
    pub step_executor_constraint: Option<PlanStepExecutorKind>,
    /// 多角色工作台：本回合工具白名单；`None` 不限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// Web `/chat*`：结构化日志根 span（`job_id` / `conversation_id` / 外层轮次 / 当前工具）；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    /// 当前编排子阶段（供失败时 SSE `sub_phase` 与日志）；由 `outer_loop` / 分阶段路径在调用模型或执行工具前更新。
    pub sub_phase: AgentTurnSubPhase,
}

impl RunLoopParams<'_> {
    /// 当前回合的 SSE/终端/流式/取消开关，供 [`crate::llm::CompleteChatRetryingParams::new`] 与 [`super::agent_llm_call::AgentLlmCall`] 复用。
    #[inline]
    pub(crate) fn llm_transport_opts(&self) -> crate::llm::LlmRetryingTransportOpts<'_> {
        crate::llm::LlmRetryingTransportOpts {
            out: self.out,
            render_to_terminal: self.render_to_terminal,
            no_stream: self.no_stream,
            cancel: self.cancel,
            plain_terminal_stream: self.plain_terminal_stream,
        }
    }
}
