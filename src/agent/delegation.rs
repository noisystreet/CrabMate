//! 子 Agent 委托的**实现层**（#839 P1 第 2 步）。
//!
//! 契约在 `crate::cm_tools::delegation_host`；本模块把 `SubTaskRequest` 装配成
//! **隔离的**子回合（经 `SubTurnSpec`，P0-4 已落地）并跑 `TurnEngine`。
//!
//! 三条不变量（设计 `docs/design/subagent_delegation.md` §5 / §7）：
//!
//! 1. **隔离**：子回合用全新 messages、不写父 SSE 流、共享父的 `cancel`——
//!    全部由 `SubTurnSpec::into_turn_request` 一次性落实，本模块不重复做。
//! 2. **掐断不传染**：子任务预算耗尽只体现为一条 `status = budget_exhausted` 的
//!    `SubTaskReport`，**本方法仍返回 `Ok`**。父回合要不要停是父的事。
//! 3. **不可提权**：权限取 `min` 链结果（由调用方算好传入），工具集按只读派生后收窄。
//!
//! **首版限制**：`max_tool_calls` 无法在本层限制——`TurnBudgetCounter` 没有工具调用维度
//! （它只有 LLM 次数 / Token / 墙钟），而工具账本是进程级累计。故按设计 §7 以墙钟兜底，
//! 待 `TurnBudgetCounter` 增加该维度后再接。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use async_trait::async_trait;

use crate::cm_names as tool_names;

use crate::agent::agent_turn::host::errors::{RunAgentTurnError, TurnAbortReason};
use crate::agent::sub_turn::{SubTurnMessages, SubTurnSpec};
use crate::cm_agent::permission::TurnPermissionStages;
use crate::cm_tools::delegation_host::{
    SubAgentDelegator, SubTaskLimits, SubTaskReport, SubTaskRequest, SubTaskStats, SubTaskStatus,
};
use crate::cm_tools::tool_result::{ToolError, ToolFailureCategory};
use crate::cm_types::{Message, Tool, message_content_as_str};
use crate::config::AgentConfig;
use crate::process_handles::TurnProcessHandles;
use crate::read_file_turn_cache::ReadFileTurnCache;
use crate::turn_runner::{TurnEngine, TurnStats};

/// 构造 [`CrabmateDelegator`] 的入参。
///
/// 收成一个结构体而不是十几个位置参数：这些值全部来自父回合的既有上下文，
/// 接线点（`tool_execution_host`）只需按字段填，不会有「第 7 个参数是什么」的问题。
pub(crate) struct DelegatorParts<'a> {
    pub engine: Arc<dyn TurnEngine>,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    /// 父回合已按只读性收窄后的工具定义。
    ///
    /// **调用方必须先过 `registry_policy::is_readonly_tool`**：本结构不做二次校验，
    /// 传全量工具进来就等于让子任务拥有写权限。收窄是接线点（`tool_execution_host`）
    /// 的责任——那里同时持有 `cfg` 与工具表，是唯一能正确派生只读子集的地方。
    pub tools: &'a [Tool],
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub permission: TurnPermissionStages,
    pub allowed_tool_names: Option<Arc<std::collections::HashSet<String>>>,
    pub process_handles: Arc<TurnProcessHandles>,
    pub read_file_turn_cache: Option<Arc<ReadFileTurnCache>>,
    /// 会话工作区变更集（父/子按 scope 共享同一份）；`None` = 该功能关闭。
    ///
    /// 用来在子回合前后取差集得到 `key_paths`——见 [`crate::cm_tools::workspace::changelist::WorkspaceChangelist::touched_paths`]。
    pub workspace_changelist:
        Option<Arc<crate::cm_tools::workspace::changelist::WorkspaceChangelist>>,
    /// 子回合的 system：父按需给「收窄版」，**不是**父 system 的副本。
    pub system_prompt: &'a str,
    pub cancel: Option<Arc<AtomicBool>>,
    pub long_term_memory:
        Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    pub request_audit: Option<Arc<crate::WebRequestAudit>>,
}

/// 生产实现的子 Agent 委托器。
pub(crate) struct CrabmateDelegator {
    engine: Arc<dyn TurnEngine>,
    client: reqwest::Client,
    api_key: String,
    cfg: Arc<AgentConfig>,
    tools: Vec<Tool>,
    effective_working_dir: PathBuf,
    workspace_is_set: bool,
    permission: TurnPermissionStages,
    allowed_tool_names: Option<Arc<std::collections::HashSet<String>>>,
    process_handles: Arc<TurnProcessHandles>,
    read_file_turn_cache: Option<Arc<ReadFileTurnCache>>,
    workspace_changelist: Option<Arc<crate::cm_tools::workspace::changelist::WorkspaceChangelist>>,
    system_prompt: String,
    cancel: Option<Arc<AtomicBool>>,
    long_term_memory: Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    request_audit: Option<Arc<crate::WebRequestAudit>>,
}

/// 派生交给子回合的工具集：**只读子集，且显式排除 `delegate_task` 自己**。
///
/// 两道保险：
/// 1. `is_readonly_tool` 过滤（`delegate_task` 已列入写效应名单，本就会落选）；
/// 2. **显式排除本工具名**——即便将来名单出错，也不会让子任务再派子任务。
///
/// 首版不支持嵌套（`depth` 恒 0，没有运行期深度计数），所以这条排除是**递归的唯一防线**，
/// 不能只依赖名单正确。
pub(crate) fn readonly_subset_for_subturn(
    cfg: &AgentConfig,
    tools: &[Tool],
) -> Vec<Tool> {
    tools
        .iter()
        .filter(|t| t.function.name != tool_names::DELEGATE_TASK)
        .filter(|t| crate::cm_tools::registry_policy::is_readonly_tool(cfg, &t.function.name))
        .cloned()
        .collect()
}

/// 由父回合**已生效**的能力快照反推子回合的四档输入。
///
/// 父的 `caps.level()` 已经过完整 `min` 链（角色上限 / 会话模式 / 请求 / 持久意图），
/// 因此子回合用 `(None, session_mode, Some(父级别), None)` 再跑一次 `min` 链，
/// 结果必然 **≤ 父的生效等级** —— 这就是「不可提权」，且不依赖把 stages 逐层透传。
pub(crate) fn stages_from_parent_caps(
    caps: &dyn crate::cm_agent::permission::PolicyEngine,
    session_mode: crate::cm_types::SessionMode,
) -> TurnPermissionStages {
    TurnPermissionStages {
        role_max: None,
        session_mode,
        requested: Some(caps.level()),
        persisted: None,
    }
}

impl CrabmateDelegator {
    pub(crate) fn new(parts: DelegatorParts<'_>) -> Self {
        Self {
            engine: parts.engine,
            client: parts.client.clone(),
            api_key: parts.api_key.to_string(),
            cfg: Arc::clone(parts.cfg),
            tools: parts.tools.to_vec(),
            effective_working_dir: parts.effective_working_dir.to_path_buf(),
            workspace_is_set: parts.workspace_is_set,
            permission: parts.permission,
            allowed_tool_names: parts.allowed_tool_names,
            process_handles: parts.process_handles,
            read_file_turn_cache: parts.read_file_turn_cache,
            workspace_changelist: parts.workspace_changelist,
            system_prompt: parts.system_prompt.to_string(),
            cancel: parts.cancel,
            long_term_memory: parts.long_term_memory,
            long_term_memory_scope_id: parts.long_term_memory_scope_id,
            tracing_chat_turn: parts.tracing_chat_turn,
            request_audit: parts.request_audit,
        }
    }

    /// 把父配置收窄成子回合预算：**只能减不能加**。
    ///
    /// 取「更严的一侧」而不是直接覆盖：若父回合本身配得更紧（例如父 `max_llm_calls_per_turn = 5`
    /// 而子任务默认 8），覆盖会**放宽**子任务预算，等于给了子任务父没有的额度——与
    /// 「不可提权」冲突。`0` 在本仓语义是「不限制」，故不能参与 `min`：
    /// 父不限（0）时用子的值；子写 0 时仍用父的值（不放宽）。
    ///
    /// `max_tool_calls` 在此无处施加（见模块头说明），故只收窄 LLM 次数与墙钟。
    fn narrowed_cfg(&self, limits: &SubTaskLimits) -> AgentConfig {
        let mut cfg = (*self.cfg).clone();
        cfg.turn_budget.max_llm_calls_per_turn = tighter(
            self.cfg.turn_budget.max_llm_calls_per_turn,
            limits.max_llm_calls,
        );
        cfg.turn_budget.max_turn_duration_seconds = tighter(
            self.cfg.turn_budget.max_turn_duration_seconds,
            limits.max_wall_secs,
        );
        cfg
    }
}

/// 两个上限里更严的一个，`0` 视为「不限制」。
///
/// - 父不限（0）→ 用子的值；
/// - 子写 0（不限制）→ 仍用父的值，**不放宽**；
/// - 其余取较小者。
fn tighter<T: Copy + Default + Ord>(parent: T, child: T) -> T {
    let zero = T::default();
    if parent == zero {
        child
    } else if child == zero {
        parent
    } else {
        parent.min(child)
    }
}

/// 子回合 messages 里最后一条 assistant 正文，作为 `summary` 的来源。
fn last_assistant_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.tool_calls.as_ref().is_none_or(|t| t.is_empty()))
        .and_then(|m| message_content_as_str(&m.content))
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 回合错误 → 子任务终态。
///
/// 关键：**超预算不是失败**。它表示「子任务被掐断」，父回合照常结束即可（设计 §7）。
fn status_for_error(err: &RunAgentTurnError) -> SubTaskStatus {
    match err {
        RunAgentTurnError::TimeLimitExhausted { .. } => SubTaskStatus::BudgetExhausted,
        RunAgentTurnError::TurnAborted { reason, .. } => match reason {
            TurnAbortReason::UserCancelled | TurnAbortReason::SseDisconnected => {
                SubTaskStatus::Cancelled
            }
        },
        // LLM 次数上限经 `Other { message }` 透传（turn_budget 的既定映射）；
        // 注意不要嗅探任意 `Other` 文案，只认这一个明确前缀。
        RunAgentTurnError::Other { message, .. } if message.contains("LLM 调用次数上限") => {
            SubTaskStatus::BudgetExhausted
        }
        _ => SubTaskStatus::Failed,
    }
}

fn error_code_for_status(status: SubTaskStatus) -> Option<&'static str> {
    match status {
        SubTaskStatus::Ok => None,
        SubTaskStatus::BudgetExhausted => Some("subtask_budget_exhausted"),
        SubTaskStatus::Cancelled => Some("subtask_cancelled"),
        SubTaskStatus::Denied => Some("subtask_denied"),
        SubTaskStatus::Failed => Some("subtask_failed"),
    }
}

fn stats_from(budget: Option<&TurnStats>, fallback_elapsed_secs: u64) -> SubTaskStats {
    SubTaskStats {
        llm_calls: budget.map_or(0, |s| s.llm_calls),
        // `TurnStats` 刻意不含工具调用次数（进程级累计，无单轮维度）；
        // 与设计 §13 的「父子预算口径分离」一致，这里就报 0 而不是编一个近似值。
        tool_calls: 0,
        // 成功时以回合账本的墙钟为准（它才是子回合的真实耗时）；
        // 失败路径 `TurnOutcome` 不携带聚合，退回委托层的局部计时。
        elapsed_secs: budget.map_or(fallback_elapsed_secs, |s| s.elapsed_secs),
        estimated_tokens: budget.map_or(0, |s| s.estimated_tokens) as u64,
    }
}

#[async_trait]
impl SubAgentDelegator for CrabmateDelegator {
    async fn run(&self, req: SubTaskRequest) -> Result<SubTaskReport, ToolError> {
        if !req.prompt_is_usable() {
            return Err(ToolError {
                category: ToolFailureCategory::InvalidInput,
                code: "subtask_empty_prompt".to_string(),
                message: "子任务 prompt 为空".to_string(),
                retryable: false,
                parsed: crate::cm_tools::tool_result::ParsedToolOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: None,
                },
            });
        }
        if let Some(cancel) = self.cancel.as_deref()
            && cancel.load(Ordering::Relaxed)
        {
            return Ok(
                SubTaskReport::bounded(
                    SubTaskStatus::Cancelled,
                    "父回合已取消，子任务未开始",
                    Vec::new(),
                    SubTaskStats::default(),
                    req.max_report_chars,
                )
                .with_error_code("subtask_cancelled"),
            );
        }

        let started = Instant::now();
        // 子回合**前**的已触碰路径；跑完取差集即得「这次子任务改动了哪些文件」。
        // 父子共享同一 changelist（按 scope 从 registry 取），而 `run()` 是 await 串行的，
        // 父回合在子任务期间不会并发写盘，所以差集准确。
        let paths_before: std::collections::HashSet<String> = self
            .workspace_changelist
            .as_ref()
            .map(|c| c.touched_paths().into_iter().collect())
            .unwrap_or_default();
        let sub_cfg = Arc::new(self.narrowed_cfg(&req.limits));
        let mut window = SubTurnMessages::new();
        let turn_req = SubTurnSpec {
            client: &self.client,
            api_key: &self.api_key,
            cfg: &sub_cfg,
            system_prompt: &self.system_prompt,
            prompt: &req.prompt,
            effective_working_dir: &self.effective_working_dir,
            workspace_is_set: self.workspace_is_set,
            tools: &self.tools,
            permission: self.permission,
            allowed_tool_names: self.allowed_tool_names.clone(),
            cancel: self.cancel.clone(),
            long_term_memory: self.long_term_memory.clone(),
            long_term_memory_scope_id: self.long_term_memory_scope_id.clone(),
            process_handles: Arc::clone(&self.process_handles),
            tracing_chat_turn: self.tracing_chat_turn.clone(),
            request_audit: self.request_audit.clone(),
            read_file_turn_cache: self.read_file_turn_cache.clone(),
            // 设计 §8 要求子回合走同一条 `dispatch_tool`，审批因此自动沿用。
            // P1 首版不给审批通道：需审批的调用会直接失败（比「静默放行」安全）。
            web_tool_ctx: None,
            // 首版不透传父的 llm_backend：默认 `TurnEngine` 会按同一 `cfg` 自建后端。
            // 透传需要 `&'static` 后端，与委托器的 `Send + Sync` 长期持有相冲突；
            // 若将来要「子回合复用父的具体后端实例」，应改为持 `Arc<dyn ChatCompletionsBackend>`。
            llm_backend: None,
        }
        .into_turn_request(&mut window);

        let outcome = self.engine.run(turn_req).await;
        let elapsed_secs = started.elapsed().as_secs();

        let (status, stats, summary) = match &outcome {
            Ok(out) => (
                SubTaskStatus::Ok,
                stats_from(Some(&out.stats), elapsed_secs),
                last_assistant_text(window.as_slice()),
            ),
            Err(e) => (
                status_for_error(e),
                stats_from(None, elapsed_secs),
                format!("子任务未正常结束：{e}"),
            ),
        };

        // **父回合照常结束**：无论子任务成败，本方法都返回 Ok(report)。
        // 只取**本次子任务新增**的写路径；只读探索不会有新增 → 空（语义正确）。
        let key_paths: Vec<String> = self
            .workspace_changelist
            .as_ref()
            .map(|c| {
                c.touched_paths()
                    .into_iter()
                    .filter(|p| !paths_before.contains(p))
                    .collect()
            })
            .unwrap_or_default();
        let mut report = SubTaskReport::bounded(
            status,
            summary,
            key_paths,
            stats,
            req.max_report_chars,
        );
        if let Some(code) = error_code_for_status(status) {
            report = report.with_error_code(code);
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::host::errors::AgentTurnSubPhase;
    use crate::turn_runner::TurnOutcome;

    /// 固定返回某种结局的 mock engine：用于验证「掐断不传染父回合」。
    struct FixedEngine(Result<TurnOutcome, Box<dyn Fn() -> RunAgentTurnError + Send + Sync>>);

    #[async_trait]
    impl TurnEngine for FixedEngine {
        async fn run(
            &self,
            _params: crate::TurnRequest<'_>,
        ) -> Result<TurnOutcome, RunAgentTurnError> {
            match &self.0 {
                Ok(out) => Ok(out.clone()),
                Err(make) => Err(make()),
            }
        }
    }

    fn ok_outcome() -> TurnOutcome {
        TurnOutcome {
            commit: crate::agent::model_context_view::StructuralCommit::default(),
            stats: TurnStats {
                llm_calls: 3,
                outer_loop_iterations: 2,
                estimated_tokens: 1234,
                elapsed_secs: 5,
                degraded: false,
                truncated: false,
            },
        }
    }

    fn parts_with(engine: Arc<dyn TurnEngine>) -> DelegatorParts<'static> {
        // 泄漏少量测试夹具，换取 `'static` 简化：测试进程内可接受。
        let cfg: &'static Arc<AgentConfig> = Box::leak(Box::new(Arc::new(
            crate::cm_config::load_config(None).expect("嵌入默认配置应可加载"),
        )));
        let client: &'static reqwest::Client = Box::leak(Box::new(reqwest::Client::new()));
        DelegatorParts {
            engine,
            client,
            api_key: "test-key",
            cfg,
            tools: &[],
            effective_working_dir: std::path::Path::new("."),
            workspace_is_set: true,
            permission: TurnPermissionStages {
                role_max: None,
                session_mode: crate::cm_types::SessionMode::Act,
                requested: None,
                persisted: None,
            },
            allowed_tool_names: None,
            process_handles: TurnProcessHandles::default_arc(),
            read_file_turn_cache: None,
            workspace_changelist: None,
            system_prompt: "你是子 Agent",
            cancel: None,
            long_term_memory: None,
            long_term_memory_scope_id: None,
            tracing_chat_turn: None,
            request_audit: None,
        }
    }

    fn request() -> SubTaskRequest {
        SubTaskRequest {
            prompt: "找出 src 下的所有 TODO".to_string(),
            depth: 0,
            allowed_tools: None,
            limits: SubTaskLimits::default(),
            max_report_chars: 200,
        }
    }

    /// **设计 §11 的核心验收**：子任务被掐断，父回合仍拿到 `Ok`。
    #[tokio::test]
    async fn budget_exhausted_does_not_propagate_to_parent() {
        let engine = Arc::new(FixedEngine(Err(Box::new(|| {
            RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Planner,
                message: "已达到单轮墙钟时间上限 (300秒)".to_string(),
            }
        }))));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("父回合必须拿到 Ok");
        assert_eq!(report.status, SubTaskStatus::BudgetExhausted);
        assert_eq!(report.error_code.as_deref(), Some("subtask_budget_exhausted"));
        assert!(!report.status.is_ok());
    }

    /// 用户取消 → `Cancelled`，同样不传染。
    #[tokio::test]
    async fn user_cancel_maps_to_cancelled() {
        let engine = Arc::new(FixedEngine(Err(Box::new(|| {
            RunAgentTurnError::TurnAborted {
                phase: AgentTurnSubPhase::Planner,
                reason: TurnAbortReason::UserCancelled,
            }
        }))));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("Ok");
        assert_eq!(report.status, SubTaskStatus::Cancelled);
    }

    /// LLM 次数上限走 `Other` 文案，必须映射为 `BudgetExhausted` 而非 `Failed`。
    #[tokio::test]
    async fn llm_call_limit_maps_to_budget_exhausted() {
        let engine = Arc::new(FixedEngine(Err(Box::new(|| RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "已达到单轮 LLM 调用次数上限 (8)".to_string(),
        }))));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("Ok");
        assert_eq!(report.status, SubTaskStatus::BudgetExhausted);
    }

    /// 其它错误 → `Failed`（不与「超预算」混淆）。
    #[tokio::test]
    async fn other_error_maps_to_failed() {
        let engine = Arc::new(FixedEngine(Err(Box::new(|| RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "缺 tool_calls".to_string(),
        }))));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("Ok");
        assert_eq!(report.status, SubTaskStatus::Failed);
        assert_eq!(report.error_code.as_deref(), Some("subtask_failed"));
    }

    /// 正常结束 → `Ok`，且 stats 与 summary 有界。
    #[tokio::test]
    async fn ok_outcome_reports_stats() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("Ok");
        assert_eq!(report.status, SubTaskStatus::Ok);
        assert_eq!(report.stats.llm_calls, 3);
        assert_eq!(report.stats.elapsed_secs, 5);
        assert_eq!(report.stats.estimated_tokens, 1234);
        assert!(report.error_code.is_none());
    }

    /// 空 prompt 在装配之前就拒掉——不白烧一次 LLM 往返。
    #[tokio::test]
    async fn empty_prompt_is_rejected_before_any_llm_call() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let d = CrabmateDelegator::new(parts_with(engine));
        let mut req = request();
        req.prompt = "   ".to_string();
        let err = d.run(req).await.expect_err("应拒绝");
        assert_eq!(err.code, "subtask_empty_prompt");
    }

    /// 父已取消 → 直接返回 `Cancelled`，不启动子回合。
    #[tokio::test]
    async fn parent_cancel_short_circuits() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let mut p = parts_with(engine);
        p.cancel = Some(Arc::new(AtomicBool::new(true)));
        let d = CrabmateDelegator::new(p);
        let report = d.run(request()).await.expect("Ok");
        assert_eq!(report.status, SubTaskStatus::Cancelled);
        assert_eq!(report.stats.llm_calls, 0, "不应发起任何 LLM 调用");
    }

    /// **`key_paths` 来自 changelist 前后差集**，且只含本次新增。
    ///
    /// 首版该字段恒为 `Vec::new()`（验收第 2 条只做了「摘要」一半）。
    #[tokio::test]
    async fn key_paths_are_the_delta_of_this_subturn() {
        use crate::cm_tools::workspace::changelist::WorkspaceChangelist;

        let cl = Arc::new(WorkspaceChangelist::default());
        // 父回合先改过一个文件——它**不该**出现在子任务的 key_paths 里。
        cl.record_mutation("already/by-parent.rs", None, Some("x".into()));

        struct MutatingEngine(Arc<WorkspaceChangelist>);
        #[async_trait]
        impl crate::turn_runner::TurnEngine for MutatingEngine {
            async fn run(
                &self,
                _p: crate::TurnRequest<'_>,
            ) -> Result<crate::turn_runner::TurnOutcome, RunAgentTurnError> {
                // 模拟子回合写盘。
                self.0.record_mutation("sub/new.rs", None, Some("y".into()));
                Ok(ok_outcome())
            }
        }

        let mut p = parts_with(Arc::new(MutatingEngine(Arc::clone(&cl))));
        p.workspace_changelist = Some(Arc::clone(&cl));
        let d = CrabmateDelegator::new(p);
        let report = d.run(request()).await.expect("Ok");

        assert_eq!(
            report.key_paths,
            vec!["sub/new.rs".to_string()],
            "只应包含本次子任务新增的路径，不含父回合先前的写入"
        );
    }

    /// 没有 changelist（功能关闭）→ `key_paths` 为空，不 panic。
    #[tokio::test]
    async fn key_paths_empty_without_changelist() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let d = CrabmateDelegator::new(parts_with(engine));
        let report = d.run(request()).await.expect("Ok");
        assert!(report.key_paths.is_empty());
    }

    /// **递归防线**：子任务的工具集绝不能包含 `delegate_task` 自己。
    ///
    /// 首版没有运行期深度计数（`depth` 恒 0），所以这条是唯一防线。
    /// 两道保险都要验：显式排除 + 写效应名单。
    #[test]
    fn subturn_toolset_never_contains_delegate_task() {
        let cfg = crate::cm_config::load_config(None).expect("嵌入默认配置应可加载");
        let mk = |name: &str| crate::cm_types::Tool {
            typ: "function".to_string(),
            function: crate::cm_types::FunctionDef {
                name: name.to_string(),
                description: String::new(),
                parameters: serde_json::json!({"type": "object"}),
            },
        };
        let parent = vec![
            mk(tool_names::DELEGATE_TASK),
            mk(tool_names::READ_FILE),
            mk(tool_names::RUN_COMMAND),
        ];
        let subset = self::readonly_subset_for_subturn(&cfg, &parent);
        let names: Vec<&str> = subset.iter().map(|t| t.function.name.as_str()).collect();
        assert!(
            !names.contains(&tool_names::DELEGATE_TASK),
            "子任务工具集不得含 delegate_task（否则无限递归）：{names:?}"
        );
        assert!(names.contains(&tool_names::READ_FILE), "只读工具应保留");
        assert!(!names.contains(&tool_names::RUN_COMMAND), "写/执行类应排除");
    }

    /// 写效应名单必须含 `delegate_task`（第一道保险）。
    #[test]
    fn delegate_task_is_write_effect() {
        assert!(
            crate::cm_config::tool_write_effect::builtin_write_effect_tools()
                .contains(tool_names::DELEGATE_TASK),
            "delegate_task 会跑整个子回合，不是只读操作"
        );
    }

    /// **父回合配得更紧时，子任务必须跟父走**——这是「不可提权」的关键一条。
    ///
    /// 首版此处直接覆盖成 `limits`，等于给子任务父没有的额度。
    #[test]
    fn narrowed_cfg_respects_tighter_parent_budget() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let mut p = parts_with(engine);
        // 父回合故意配得比子任务默认值更紧。
        let mut parent_cfg = (**p.cfg).clone();
        parent_cfg.turn_budget.max_llm_calls_per_turn = 5;
        parent_cfg.turn_budget.max_turn_duration_seconds = 60;
        p.cfg = Box::leak(Box::new(Arc::new(parent_cfg)));

        let d = CrabmateDelegator::new(p);
        let narrowed = d.narrowed_cfg(&SubTaskLimits {
            max_llm_calls: 8,
            max_tool_calls: 16,
            max_wall_secs: 300,
        });
        assert_eq!(
            narrowed.turn_budget.max_llm_calls_per_turn, 5,
            "父更紧时必须跟父，不能放宽到 8"
        );
        assert_eq!(
            narrowed.turn_budget.max_turn_duration_seconds, 60,
            "父更紧时必须跟父，不能放宽到 300"
        );
    }

    /// `0` 表示「不限制」，不能参与 `min`：父不限→用子的；子写 0→仍用父的。
    #[test]
    fn tighter_handles_zero_as_unlimited() {
        assert_eq!(tighter(0, 8), 8, "父不限 → 用子的");
        assert_eq!(tighter(5, 0), 5, "子写 0 也不能放宽到不限");
        assert_eq!(tighter(5, 8), 5, "取更严");
        assert_eq!(tighter(8, 5), 5, "取更严（对称）");
        assert_eq!(tighter(0, 0), 0, "两者都不限");
    }
}
