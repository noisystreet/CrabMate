//! Web/CLI 共用：外层循环的运行期参数。
//!
//! 可变状态归属（TurnState / PerCoord / Budget / Flight）见
//! **`docs/design/run_loop_state_ownership.md`**。
//!
//! **`RunLoopCtx`**：整场固定的输入上下文，按职责分为四块（降低扁平字段带来的隐式耦合）：
//! - [`RunLoopCore`]：LLM 接入、配置快照、工具表与工作目录；
//! - [`RunLoopIo`]：取消 / `no_stream`，以及嵌套的 [`super::TurnControlSink`]；
//! - [`RunLoopAttach`]：工具运行时句柄、缓存、记忆；
//! - [`RunLoopObs`]：Chrome trace、结构化 tracing、HTTP 审计、[`crate::process_handles::TurnProcessHandles`]。
//!
//! **`RunLoopTurnState`**：可变会话状态与本回合决策覆盖（私有 **`messages_buf`**、**`messages()`** / **`messages_buffer_mut()`**、**`messages_revision`**、`sub_phase`、模型/温度覆盖、[`TurnPlannerHints`] 等）。
//! **勿**在此袋存放终答 Gate / 工作流反思 / 外循环 pre-gate 计数（那些在 `PerCoordinator`）。
//!
//! **`messages_revision`**：在每次**就地**改写消息缓冲、以及每次 [`crate::agent::context_window::prepare_messages_for_model`] 完成后递增（单调；
//! 可与 `PerCoordinator` 的 workflow_validate 层缓存失效语义对照排障）。
//!
//! **`RunLoopParams`**：二者合一，供 `run_agent_turn_common` 与各子模块持有单一句柄。
//!
//! **[`OuterLoopPlanCallModelRole`]**：单 Agent **`outer_loop`** 每次 **P** 步选用 planner 端点还是 executor 端点（与 `iteration_count` 对应关系集中在一处）。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::agent::turn_budget::TurnBudgetCounter;

use crate::workspace::changelist::WorkspaceChangelist;

use super::errors::AgentTurnSubPhase;
use super::turn_sink::TurnControlSink;
use crate::PerTurnFlight;
use crate::WebRequestAudit;
use crate::agent::agent_turn::messages::{
    insert_separator_after_last_user_for_turn, push_assistant_merging_trailing_empty_placeholder,
};
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::tool_registry;
use crate::types::{LlmSeedOverride, Message};

/// LLM 接入、配置快照与工作目录（整场不变）。
pub(crate) struct RunLoopCore<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
}

/// SSE/流式、取消与 CLI/TUI 侧回调（传输语义）。
///
/// 控制面见 [`TurnControlSink`]（`turn_sink`）。
pub(crate) struct RunLoopIo<'a> {
    pub no_stream: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    pub control: TurnControlSink<'a>,
}

/// 工具运行时、缓存、记忆与分阶段冻结开关（执行附件）。
pub(crate) struct RunLoopAttach<'a> {
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    pub per_flight: Option<Arc<PerTurnFlight>>,
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    /// `conversation_id` 或 CLI 固定 `cli`；`None` 时不按会话隔离（跳过记忆）。
    pub long_term_memory_scope_id: Option<String>,
    /// MCP stdio 多服务器回合句柄；`None` 时不处理 `mcp__*` 工具名。
    pub mcp_turn: Option<crate::mcp::McpTurnHandle>,
    /// 单轮内 `read_file` 磁盘缓存；`None` 且配置启用时由 `run_agent_turn` 创建。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    /// 本会话工作区变更集；`None` 时不记录/不注入（见 `session_workspace_changelist_*` 配置）。
    pub workspace_changelist: Option<Arc<WorkspaceChangelist>>,
    /// 多角色工作台：本回合工具白名单；`None` 不限制。
    pub turn_allowed_tool_names: Option<Arc<HashSet<String>>>,
    /// 本回合会话工作模式（Ask/Plan/Act）；门控之后挂只读约束。
    pub session_mode: crate::cm_types::SessionMode,
}

/// Chrome trace、结构化 tracing、HTTP 审计与进程级句柄。
pub(crate) struct RunLoopObs {
    /// 整请求 Chrome trace（`CM_REQUEST_CHROME_TRACE_DIR`）；`None` 关闭。
    pub request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    /// Web `/chat*`：结构化日志根 span（`job_id` / `conversation_id` / 外层轮次 / 当前工具）；CLI 等为 `None`。
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    /// Web：HTTP 审计；非 Web 为 `None`。
    pub request_audit: Option<Arc<WebRequestAudit>>,
    /// 回合句柄：工具统计记录器等（[`TurnProcessHandles`](crate::process_handles::TurnProcessHandles)；与 [`crate::RunAgentTurnParams::obs`] 的 `process_handles` 同源）。
    pub process_handles: Arc<crate::process_handles::TurnProcessHandles>,
    /// per-step trace sink（bench/e2e 注入；`None` 时零开销）。
    /// 由 [`crate::AgentTurnTransport::trace_sink`] 传入，供 LLM 请求/响应、工具调用前后 emit
    /// [`crate::cm_llm::TraceEvent`]。
    pub trace_sink: Option<Arc<dyn crate::cm_llm::TraceSink>>,
    /// 后台任务注册表（`run_command` 的 `async=true`）；`None` 时返回未启用。
    pub tool_job_registry: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

/// 单轮 `run_agent_turn` 内相对稳定的一侧（整场不应再混入会话可变字段）。
pub(crate) struct RunLoopCtx<'a> {
    pub core: RunLoopCore<'a>,
    pub io: RunLoopIo<'a>,
    pub attach: RunLoopAttach<'a>,
    pub obs: RunLoopObs,
}

/// 单轮 planner / Act 句执行约束相关的**附加约束**（与 `messages` 正交），集中存放以避免 `RunLoopTurnState` 顶层散落布尔与 `Option`。
///
/// - **执行约束临时 system**：Act 句启发式命中只读约束时，在首轮 P 前注入（见 [`crate::types::Message::system_execution_constraint_hint`]）。
/// - **分步子代理**：当前步 `executor_kind` 收窄可见工具（常规外环为 `None`）。
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnPlannerHints {
    pub(crate) execution_constraint_hint: Option<String>,
    pub(crate) step_executor_constraint: Option<PlanStepExecutorKind>,
    /// 本回合起点启发式快照（供 [`TurnRouteDecisionV1`] 组装）。
    pub(crate) turn_start_snapshot: Option<crate::cm_agent::agent_turn::TurnStartSnapshot>,
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
    /// 首轮 P 前注入的执行约束临时 system（消费后即清空）。
    pub(crate) fn take_execution_constraint_hint(&mut self) -> Option<String> {
        self.execution_constraint_hint.take()
    }
}

/// 会话与编排可变侧：**消息缓冲**、失败时的 **`sub_phase`**、模型覆盖与本步 `executor_kind` 等。
pub(crate) struct RunLoopTurnState<'a> {
    /// 装配入口（`lib` / 测试）写入；业务路径请优先用 [`RunLoopTurnState::messages`] / [`RunLoopTurnState::messages_buffer_mut`]。
    pub(crate) messages_buf: &'a mut Vec<Message>,
    /// 单调递增：任意消息缓冲变异或一次「发往模型前」[`crate::agent::context_window::prepare_messages_for_model`] 完成后 +1（`wrapping`）。
    pub(crate) messages_revision: u64,
    /// 当前编排子阶段（供失败时 SSE `sub_phase` 与日志）；由 `outer_loop` / 分阶段路径在调用模型或执行工具前更新。
    pub sub_phase: AgentTurnSubPhase,
    /// Act 句执行约束与分步子代理约束（见 [`TurnPlannerHints`]）。
    pub(crate) turn_planner_hints: TurnPlannerHints,
    /// `None` 时使用 `cfg.llm_sampling.temperature`。
    pub temperature_override: Option<f32>,
    /// 覆盖本回合的 `model`（`None` 时使用 `cfg.llm.model` / planner_model）
    pub model_override: Option<String>,
    /// 若为 `true`，LLM 调用时使用 `cfg.llm.executor_model` 而非 `cfg.llm.planner_model`。
    pub use_executor_model: bool,
    /// 执行阶段模型覆盖（当 use_executor_model 为 true 时优先于 cfg.llm.executor_model）
    pub executor_model_override: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_base。
    pub executor_api_base: Option<String>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_key。
    pub executor_api_key: Option<String>,
    pub seed_override: LlmSeedOverride,
    /// 单轮墙钟与 LLM 调用计数（外循环与分层 Operator 共用）。
    pub turn_budget: Arc<TurnBudgetCounter>,
    /// 本用户回合窗口/注入时间线（SSE 去重 + 落盘）。
    pub(crate) context_timeline: crate::cm_agent::context_timeline::ContextTimelineAcc,
}

impl<'a> RunLoopTurnState<'a> {
    #[inline]
    fn bump_messages_revision(&mut self) {
        self.messages_revision = self.messages_revision.wrapping_add(1);
    }

    /// 只读：当前消息缓冲（与底层 `Vec` 同源）。
    #[inline]
    pub(crate) fn messages(&self) -> &[Message] {
        self.messages_buf
    }

    /// 工具执行、SSE 分隔线注入等需持有 `&mut Vec<Message>` 的路径。
    ///
    /// 若直接改了 `Vec` 的内容而未经过 [`Self::push_message`] 等自带 bump 的 API，调用方须在返回后自行保证
    /// **`messages_revision`** 与缓存语义一致（通常下一轮 **`prepare_messages_for_model`** 会再递增修订号）。
    #[inline]
    pub(crate) fn messages_buffer_mut(&mut self) -> &mut Vec<Message> {
        self.messages_buf
    }

    /// 只读：当前缓冲代数（与当前缓冲条数无必然相等关系）。
    #[inline]
    pub(crate) fn messages_buffer_revision(&self) -> u64 {
        self.messages_revision
    }

    pub(crate) fn push_message(&mut self, msg: Message) {
        self.messages_buf.push(msg);
        self.bump_messages_revision();
    }

    pub(crate) fn pop_message(&mut self) -> Option<Message> {
        let r = self.messages_buf.pop();
        if r.is_some() {
            self.bump_messages_revision();
        }
        r
    }

    #[cfg(test)]
    pub(crate) fn truncate_messages(&mut self, len: usize) {
        if self.messages_buf.len() != len {
            self.messages_buf.truncate(len);
            self.bump_messages_revision();
        }
    }

    pub(crate) fn retain_messages(&mut self, mut keep: impl FnMut(&Message) -> bool) {
        let before = self.messages_buf.len();
        self.messages_buf.retain(|m| keep(m));
        if self.messages_buf.len() != before {
            self.bump_messages_revision();
        }
    }

    pub(crate) fn push_assistant_merging_trailing_empty(&mut self, msg: Message) {
        push_assistant_merging_trailing_empty_placeholder(self.messages_buf, msg);
        self.bump_messages_revision();
    }

    pub(crate) fn flush_context_timeline_markers(&mut self) {
        let markers = self.context_timeline.persist_markers();
        if markers.is_empty() {
            return;
        }
        crate::cm_agent::context_timeline::strip_context_window_timeline_markers(self.messages_buf);
        self.messages_buf.extend(markers);
        self.bump_messages_revision();
    }

    /// 本轮 user 后插入 UI 分隔线（若未插入则不变更代数）。
    pub(crate) fn insert_separator_after_last_user_for_turn(&mut self) {
        let n = self.messages_buf.len();
        insert_separator_after_last_user_for_turn(self.messages_buf);
        if self.messages_buf.len() != n {
            self.bump_messages_revision();
        }
    }

    /// 首轮 P 前注入的执行约束临时 system（消费后即清空）。
    pub(crate) fn take_execution_constraint_hint(&mut self) -> Option<String> {
        self.turn_planner_hints.take_execution_constraint_hint()
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

    /// 获取本回合 LLM 调用应使用的 model：
    /// - planner 阶段：`model_override` > `cfg.llm.planner_model` > `cfg.llm.model`
    /// - executor 阶段：`executor_model_override` > `cfg.llm.executor_model` > `cfg.llm.model`
    #[inline]
    pub(crate) fn effective_model(&self) -> Option<&str> {
        if self.turn.use_executor_model {
            self.turn
                .executor_model_override
                .as_deref()
                .or_else(|| self.ctx.core.cfg.llm.executor_model.as_deref())
        } else {
            self.turn
                .model_override
                .as_deref()
                .or_else(|| self.ctx.core.cfg.llm.planner_model.as_deref())
        }
    }

    /// 发往模型前的同步裁剪 / 可选摘要 / changelist 注入；并驱动 **`messages_revision`** 与可选 **`PerCoordinator`** 层缓存失效。
    ///
    /// 将 [`crate::agent::context_window::prepare_messages_for_model`] 与回合缓冲 + **`messages_revision`** 挂钩集中在一处，避免调用点漏传 revision。
    pub(crate) async fn prepare_turn_messages_for_model(
        &mut self,
        per_coord_layer_cache: Option<&mut crate::agent::per_coord::PerCoordinator>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let delta = crate::agent::context_window::prepare_messages_for_model(
            self.ctx.core.llm_backend,
            self.ctx.core.client,
            self.ctx.core.api_key,
            self.ctx.core.cfg.as_ref(),
            self.turn.messages_buf,
            self.ctx
                .attach
                .workspace_changelist
                .as_ref()
                .map(|a| a.as_ref()),
            crate::agent::context_window::PrepareMessagesForModelHooks {
                per_coord_layer_cache,
                run_loop_messages_revision: Some(&mut self.turn.messages_revision),
                turn_budget: Some(&self.turn.turn_budget),
            },
        )
        .await?;
        let events = self.turn.context_timeline.merge(
            crate::cm_agent::context_timeline::ContextTimelineSnapshot {
                messages: self.turn.messages_buf,
                pipeline: delta.pipeline,
                summarized: delta.summarized,
                summary_tail_kept: delta.summary_tail_kept,
            },
        );
        crate::agent::agent_turn::turn_loop::context_timeline_sse::emit_context_timeline_sse(
            &self.ctx.io.control,
            &events,
        )
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod turn_planner_hints_tests {
    use super::{OuterLoopPlanCallModelRole, TurnPlannerHints};

    #[test]
    fn take_execution_constraint_hint_drains_once() {
        let mut h = TurnPlannerHints {
            execution_constraint_hint: Some("hint".into()),
            ..Default::default()
        };
        assert_eq!(h.take_execution_constraint_hint().as_deref(), Some("hint"));
        assert!(h.take_execution_constraint_hint().is_none());
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
            messages_buf: &mut storage,
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
            turn_budget: crate::agent::turn_budget::TurnBudgetCounter::new_shared(),
            context_timeline: Default::default(),
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
