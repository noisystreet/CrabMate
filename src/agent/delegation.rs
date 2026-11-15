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
    pub tools: &'a [Tool],
    pub effective_working_dir: &'a std::path::Path,
    pub workspace_is_set: bool,
    pub permission: TurnPermissionStages,
    pub allowed_tool_names: Option<Arc<std::collections::HashSet<String>>>,
    pub process_handles: Arc<TurnProcessHandles>,
    pub read_file_turn_cache: Option<Arc<ReadFileTurnCache>>,
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
    system_prompt: String,
    cancel: Option<Arc<AtomicBool>>,
    long_term_memory: Option<Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    request_audit: Option<Arc<crate::WebRequestAudit>>,
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
    /// `max_tool_calls` 在此无处施加（见模块头说明），故只收窄 LLM 次数与墙钟。
    fn narrowed_cfg(&self, limits: &SubTaskLimits) -> AgentConfig {
        let mut cfg = (*self.cfg).clone();
        cfg.turn_budget.max_llm_calls_per_turn = limits.max_llm_calls;
        cfg.turn_budget.max_turn_duration_seconds = limits.max_wall_secs;
        cfg
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
        let mut report = SubTaskReport::bounded(
            status,
            summary,
            Vec::new(),
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

    /// 子回合预算只收窄、不放宽。
    #[test]
    fn narrowed_cfg_never_widens_budget() {
        let engine = Arc::new(FixedEngine(Ok(ok_outcome())));
        let d = CrabmateDelegator::new(parts_with(engine));
        let limits = SubTaskLimits {
            max_llm_calls: 8,
            max_tool_calls: 16,
            max_wall_secs: 300,
        };
        let narrowed = d.narrowed_cfg(&limits);
        assert_eq!(narrowed.turn_budget.max_llm_calls_per_turn, 8);
        assert_eq!(narrowed.turn_budget.max_turn_duration_seconds, 300);
    }
}
