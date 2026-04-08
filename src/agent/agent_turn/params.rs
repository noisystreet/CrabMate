//! Web/CLI 共用：外层循环与分阶段规划的运行期参数。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::workspace_changelist::WorkspaceChangelist;

use tokio::sync::mpsc;

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
}
