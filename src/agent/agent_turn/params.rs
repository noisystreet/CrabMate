//! Web/CLI 共用：外层循环与分阶段规划的运行期参数。
//!
//! **`RunLoopCtx`**：整场固定的输入上下文（HTTP 客户端、配置快照、工具表、SSE 通道、冻结的分阶段开关等）。
//! **`RunLoopTurnState`**：可变会话状态与本回合决策覆盖（`messages`、`messages_revision`、`sub_phase`、模型/温度覆盖、[`TurnPlannerHints`] 等）。
//! **`messages_revision`**：在每次**就地**改写 `messages` 缓冲、以及每次 [`crate::agent::context_window::prepare_messages_for_model`] 完成后递增（单调；
//! 可与 `PerCoordinator` 的 workflow_validate 层缓存失效语义对照排障）。
//! **`RunLoopParams`**：二者合一，供 `run_agent_turn_common` 与各子模块持有单一句柄。
//! **[`OuterLoopPlanCallModelRole`]**：单 Agent **`outer_loop`** 每次 **P** 步选用 planner 端点还是 executor 端点（与 `iteration_count` 对应关系集中在一处）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::workspace::changelist::WorkspaceChangelist;

use tokio::sync::mpsc;

use super::errors::AgentTurnSubPhase;
use super::messages::{
    insert_separator_after_last_user_for_turn, push_assistant_merging_trailing_empty_placeholder,
};
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

/// 单 Agent [`super::outer_loop::run_agent_outer_loop`] 内每次 **P** 调用对应的模型端点角色。
///
/// 将「第几轮用 planner vs executor」从隐式 `iteration_count >= 2` 收拢为显式枚举，便于 tracing 与文档对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterLoopPlanCallModelRole {
    /// 首轮及唯一一轮：走 `planner_model` / planner 覆盖；**不**应用 executor 的 `api_base` / `api_key` 覆盖。
    PlannerRound,
    /// 第二轮及以后：走 `executor_model` / executor 覆盖；可应用 `executor_api_base` / `executor_api_key`。
    ExecutorRound,
}

impl OuterLoopPlanCallModelRole {
    /// `iteration_count` 为 `run_outer_loop_single_iteration` 传入值（从 1 递增）。
    #[inline]
    pub(crate) fn from_outer_loop_iteration(iteration_count: u32) -> Self {
        if iteration_count <= 1 {
            Self::PlannerRound
        } else {
            Self::ExecutorRound
        }
    }

    /// 与 [`RunLoopTurnState::use_executor_model`] 对齐：`PlannerRound` → `false`，`ExecutorRound` → `true`。
    #[inline]
    pub(crate) fn sets_use_executor_model(self) -> bool {
        matches!(self, Self::ExecutorRound)
    }

    #[inline]
    pub(crate) fn as_trace_str(self) -> &'static str {
        match self {
            Self::PlannerRound => "planner_round",
            Self::ExecutorRound => "executor_round",
        }
    }
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
    /// 单调递增：任意 `messages` 变异或一次「发往模型前」[`crate::agent::context_window::prepare_messages_for_model`] 完成后 +1（`wrapping`）。
    pub(crate) messages_revision: u64,
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
    #[inline]
    fn bump_messages_revision(&mut self) {
        self.messages_revision = self.messages_revision.wrapping_add(1);
    }

    /// 只读：当前缓冲代数（与 [`Self::messages`] 长度无必然相等关系）。
    #[inline]
    pub(crate) fn messages_buffer_revision(&self) -> u64 {
        self.messages_revision
    }

    pub(crate) fn push_message(&mut self, msg: Message) {
        self.messages.push(msg);
        self.bump_messages_revision();
    }

    pub(crate) fn pop_message(&mut self) -> Option<Message> {
        let r = self.messages.pop();
        if r.is_some() {
            self.bump_messages_revision();
        }
        r
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        if self.messages.len() != len {
            self.messages.truncate(len);
            self.bump_messages_revision();
        }
    }

    pub(crate) fn retain_messages(&mut self, mut keep: impl FnMut(&Message) -> bool) {
        let before = self.messages.len();
        self.messages.retain(|m| keep(m));
        if self.messages.len() != before {
            self.bump_messages_revision();
        }
    }

    pub(crate) fn push_assistant_merging_trailing_empty(&mut self, msg: Message) {
        push_assistant_merging_trailing_empty_placeholder(self.messages, msg);
        self.bump_messages_revision();
    }

    /// 本轮 user 后插入 UI 分隔线（若未插入则不变更代数）。
    pub(crate) fn insert_separator_after_last_user_for_turn(&mut self) {
        let n = self.messages.len();
        insert_separator_after_last_user_for_turn(self.messages);
        if self.messages.len() != n {
            self.bump_messages_revision();
        }
    }

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
    /// 按 [`OuterLoopPlanCallModelRole`] 更新 `turn.use_executor_model`（供 **`outer_loop`** 每轮 **P** 前调用）。
    #[inline]
    pub(crate) fn apply_outer_loop_plan_call_model_role(
        &mut self,
        role: OuterLoopPlanCallModelRole,
    ) {
        self.turn.use_executor_model = role.sets_use_executor_model();
    }

    /// 供 [`super::plan::PerPlanCallModelParams`]：克隆 executor 端点覆盖（仅当 `use_executor_model` 时非空），避免 `&str` 长时间借用 `turn`。
    #[inline]
    pub(crate) fn plan_call_executor_endpoint_cloned(&self) -> (Option<String>, Option<String>) {
        if self.turn.use_executor_model {
            (
                self.turn.executor_api_base.clone(),
                self.turn.executor_api_key.clone(),
            )
        } else {
            (None, None)
        }
    }

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
    use super::{OuterLoopPlanCallModelRole, TurnPlannerHints};

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

    #[test]
    fn outer_loop_plan_role_matches_iteration_and_trace() {
        assert_eq!(
            OuterLoopPlanCallModelRole::from_outer_loop_iteration(1),
            OuterLoopPlanCallModelRole::PlannerRound
        );
        assert!(!OuterLoopPlanCallModelRole::PlannerRound.sets_use_executor_model());
        assert_eq!(
            OuterLoopPlanCallModelRole::PlannerRound.as_trace_str(),
            "planner_round"
        );

        assert_eq!(
            OuterLoopPlanCallModelRole::from_outer_loop_iteration(2),
            OuterLoopPlanCallModelRole::ExecutorRound
        );
        assert!(OuterLoopPlanCallModelRole::ExecutorRound.sets_use_executor_model());
        assert_eq!(
            OuterLoopPlanCallModelRole::ExecutorRound.as_trace_str(),
            "executor_round"
        );
    }

    #[test]
    fn messages_revision_increments_on_buffer_mutations() {
        use crate::agent::agent_turn::errors::AgentTurnSubPhase;
        use crate::types::{LlmSeedOverride, Message};

        let mut storage = vec![Message::user_only("u")];
        let mut turn = super::RunLoopTurnState {
            messages: &mut storage,
            messages_revision: 0,
            sub_phase: AgentTurnSubPhase::Planner,
            turn_planner_hints: TurnPlannerHints::default(),
            temperature_override: None,
            model_override: None,
            use_executor_model: false,
            executor_model_override: None,
            executor_api_base: None,
            executor_api_key: None,
            seed_override: LlmSeedOverride::FromConfig,
        };
        assert_eq!(turn.messages_buffer_revision(), 0);
        turn.push_message(Message::assistant_only("a"));
        assert_eq!(turn.messages_buffer_revision(), 1);
        turn.truncate_messages(1);
        assert_eq!(turn.messages_buffer_revision(), 2);
        turn.retain_messages(|_| true);
        assert_eq!(turn.messages_buffer_revision(), 2);
        turn.retain_messages(|m| m.role != "tool");
        assert_eq!(turn.messages_buffer_revision(), 2);
    }
}
