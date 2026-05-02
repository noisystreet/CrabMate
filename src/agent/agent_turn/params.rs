//! Web/CLI 共用：外层循环与分阶段规划的运行期参数。
//!
//! **`RunLoopCtx`**：整场固定的输入上下文（HTTP 客户端、配置快照、工具表、SSE 通道、冻结的分阶段开关等）。
//! **`RunLoopTurnState`**：可变会话状态与本回合决策覆盖（`messages`、`sub_phase`、模型/温度覆盖、[`TurnPlannerHints`] 等）。
//! **`RunLoopParams`**：二者合一，供 `run_agent_turn_common` 与各子模块持有单一句柄。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::workspace::changelist::WorkspaceChangelist;

use tokio::sync::mpsc;

use super::errors::AgentTurnSubPhase;
use crate::agent::hierarchy::HierarchyRunnerParams;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::tool_registry;
use crate::types::{LlmSeedOverride, Message};

/// 单轮 `run_agent_turn` 内相对稳定的一侧：**接入与配置快照**（整场不应再混入会话可变字段）。
pub(crate) struct RunLoopCtx<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
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
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    /// `conversation_id` 或 CLI 固定 `cli`；`None` 时不按会话隔离（跳过记忆）。
    pub long_term_memory_scope_id: Option<String>,
    /// MCP stdio 会话；`None` 时不处理 `mcp__*` 工具名。
    pub mcp_session: Option<Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
    /// 单轮内 `read_file` 磁盘缓存；`None` 且配置启用时由 `run_agent_turn` 创建。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    /// 本会话工作区变更集；`None` 时不记录/不注入（见 `session_workspace_changelist_*` 配置）。
    pub workspace_changelist: Option<Arc<WorkspaceChangelist>>,
    /// 整请求 Chrome trace（`CRABMATE_REQUEST_CHROME_TRACE_DIR`）；`None` 关闭。
    pub request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    pub staged_plan_optimizer_round: bool,
    /// 无「可同轮并行批处理」内建工具时是否跳过优化轮。见 `AgentConfig::staged_plan_optimizer_requires_parallel_tools`。
    pub staged_plan_optimizer_requires_parallel_tools: bool,
    /// 逻辑多规划员：首轮后的独立规划份数上限（1=关闭）。见 `AgentConfig::staged_plan_ensemble_count`。
    pub staged_plan_ensemble_count: u8,
    /// 寒暄/极短用户输入时是否跳过 ensemble。见 `AgentConfig::staged_plan_skip_ensemble_on_casual_prompt`。
    pub staged_plan_skip_ensemble_on_casual_prompt: bool,
    /// 多角色工作台：本回合工具白名单；`None` 不限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// Web `/chat*`：结构化日志根 span（`job_id` / `conversation_id` / 外层轮次 / 当前工具）；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    /// Web：HTTP 审计；非 Web 为 `None`。
    pub request_audit: Option<Arc<crate::web::audit::WebRequestAudit>>,
}

/// 单轮 planner / 意图门控相关的**附加约束**（与 `messages` 正交），集中存放以避免 `RunLoopTurnState` 顶层散落布尔与 `Option`。
///
/// - **意图时间线去重**：`intent_at_turn_start` 与 `staged_plan_intent_gate` 衔接时跳过重复 `intent_analysis`。
/// - **门控临时 system**：澄清/确认/只读路径在首轮 P 前注入（见 [`crate::types::Message::system_intent_gate_hint`]）。
/// - **分步子代理**：当前步 `executor_kind` 收窄可见工具（常规外环为 `None`）。
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnPlannerHints {
    pub(crate) suppress_duplicate_intent_timeline_once: bool,
    pub(crate) intent_turn_gate_hint: Option<String>,
    pub(crate) step_executor_constraint: Option<PlanStepExecutorKind>,
}

impl TurnPlannerHints {
    /// 首轮 P 前注入的意图门控临时 system（消费后即清空）。
    pub(crate) fn take_intent_turn_gate_hint(&mut self) -> Option<String> {
        self.intent_turn_gate_hint.take()
    }

    /// `intent_at_turn_start` 与 `staged_plan_intent_gate` 衔接：读取并清除「跳过重复时间线」标志。
    pub(crate) fn take_suppress_duplicate_intent_timeline_once(&mut self) -> bool {
        let v = self.suppress_duplicate_intent_timeline_once;
        self.suppress_duplicate_intent_timeline_once = false;
        v
    }
}

/// 会话与编排可变侧：**消息缓冲**、失败时的 **`sub_phase`**、模型覆盖与本步 `executor_kind` 等。
pub(crate) struct RunLoopTurnState<'a> {
    pub messages: &'a mut Vec<Message>,
    /// 当前编排子阶段（供失败时 SSE `sub_phase` 与日志）；由 `outer_loop` / 分阶段路径在调用模型或执行工具前更新。
    pub sub_phase: AgentTurnSubPhase,
    /// 意图门控与分步子代理约束（见 [`TurnPlannerHints`]）。
    pub(crate) turn_planner_hints: TurnPlannerHints,
    /// `None` 时使用 `cfg.temperature`。
    pub temperature_override: Option<f32>,
    /// 覆盖本回合的 `model`（`None` 时使用 `cfg.model` / planner_model）
    pub model_override: Option<String>,
    /// 若为 `true`，LLM 调用时使用 `cfg.executor_model` 而非 `cfg.planner_model`。
    pub use_executor_model: bool,
    /// 执行阶段模型覆盖（当 use_executor_model 为 true 时优先于 cfg.executor_model）
    pub executor_model_override: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_base。
    pub executor_api_base: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_key。
    pub executor_api_key: Option<String>,
    pub seed_override: LlmSeedOverride,
}

impl<'a> RunLoopTurnState<'a> {
    /// 首轮 P 前注入的意图门控临时 system（消费后即清空）。
    pub(crate) fn take_intent_turn_gate_hint(&mut self) -> Option<String> {
        self.turn_planner_hints.take_intent_turn_gate_hint()
    }

    /// `intent_at_turn_start` 与 `staged_plan_intent_gate` 衔接：读取并清除「跳过重复时间线」标志。
    pub(crate) fn take_suppress_duplicate_intent_timeline_once(&mut self) -> bool {
        self.turn_planner_hints
            .take_suppress_duplicate_intent_timeline_once()
    }
}

/// Web/CLI 共用：外层循环与分阶段规划注入共用的一套运行期参数。
pub(crate) struct RunLoopParams<'a> {
    pub ctx: RunLoopCtx<'a>,
    pub turn: RunLoopTurnState<'a>,
}

impl RunLoopParams<'_> {
    /// 装配 [`HierarchyRunnerParams`]：与 `hierarchy::run_hierarchical_agent` 内 Web 审批通道（`out_tx` / `approval_rx_shared`）提取逻辑一致，避免分层入口与其它调用点漂移。
    pub(crate) fn hierarchy_runner_params<'b>(
        &'b self,
        task: &'b str,
        primary_intent: Option<String>,
        secondary_intents: Vec<String>,
    ) -> HierarchyRunnerParams<'b> {
        let (tool_approval_out, tool_approval_rx) = if let Some(web_ctx) = self.ctx.web_tool_ctx {
            (
                Some(web_ctx.out_tx.clone()),
                Some(web_ctx.approval_rx_shared.clone()),
            )
        } else {
            (None, None)
        };
        HierarchyRunnerParams {
            task,
            cfg: self.ctx.cfg.as_ref(),
            llm_backend: self.ctx.llm_backend,
            client: Arc::new(self.ctx.client.clone()),
            api_key: self.ctx.api_key.to_string(),
            working_dir: self.ctx.effective_working_dir.to_path_buf(),
            sse_out: self.ctx.out.cloned(),
            tools_defs: self.ctx.tools_defs,
            tool_approval_out,
            tool_approval_rx,
            primary_intent,
            secondary_intents,
            intent_mode_bias_enabled: self.ctx.cfg.intent_mode_bias_enabled,
        }
    }

    /// 当前回合的 SSE/终端/流式/取消开关，供 [`crate::llm::CompleteChatRetryingParams::new`] 与 [`super::plan::AgentLlmCall`] 复用。
    #[inline]
    pub(crate) fn llm_transport_opts(&self) -> crate::llm::LlmRetryingTransportOpts<'_> {
        crate::llm::LlmRetryingTransportOpts {
            out: self.ctx.out,
            render_to_terminal: self.ctx.render_to_terminal,
            no_stream: self.ctx.no_stream,
            cancel: self.ctx.cancel,
            plain_terminal_stream: self.ctx.plain_terminal_stream,
        }
    }

    /// 获取本回合 LLM 调用应使用的 model：
    /// - planner 阶段：`model_override` > `cfg.planner_model` > `cfg.model`
    /// - executor 阶段：`executor_model_override` > `cfg.executor_model` > `cfg.model`
    #[inline]
    pub(crate) fn effective_model(&self) -> Option<&str> {
        if self.turn.use_executor_model {
            self.turn
                .executor_model_override
                .as_deref()
                .or_else(|| self.ctx.cfg.executor_model.as_deref())
        } else {
            self.turn
                .model_override
                .as_deref()
                .or_else(|| self.ctx.cfg.planner_model.as_deref())
        }
    }
}

#[cfg(test)]
mod turn_planner_hints_tests {
    use super::TurnPlannerHints;

    #[test]
    fn take_suppress_duplicate_clears_flag() {
        let mut h = TurnPlannerHints {
            suppress_duplicate_intent_timeline_once: true,
            ..Default::default()
        };
        assert!(h.take_suppress_duplicate_intent_timeline_once());
        assert!(!h.take_suppress_duplicate_intent_timeline_once());
        assert!(!h.suppress_duplicate_intent_timeline_once);
    }

    #[test]
    fn take_intent_gate_hint_drains_once() {
        let mut h = TurnPlannerHints {
            intent_turn_gate_hint: Some("hint".into()),
            ..Default::default()
        };
        assert_eq!(h.take_intent_turn_gate_hint().as_deref(), Some("hint"));
        assert!(h.take_intent_turn_gate_hint().is_none());
    }
}
