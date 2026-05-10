//! 分层执行器：按依赖层级执行子目标

use std::sync::Arc;

use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;

use super::execution_helpers::{summarize_subgoal_evidence, trim_for_detail};
use super::task::{TaskResult, TaskStatus};
use crate::types::{CommandApprovalDecision, Tool};
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::Receiver;

pub use super::execution_error::ExecutionError;

/// 分层执行结果
#[derive(Debug, Clone)]
pub struct HierarchicalExecutionResult {
    pub results: Vec<TaskResult>,
    pub total_duration_ms: u64,
    pub total_completed: usize,
    pub total_failed: usize,
    /// 子目标级 `acceptance.expect_output_contains` 快照（按 goal_id）。
    pub goal_expected_outputs: std::collections::HashMap<String, Vec<String>>,
}

/// 分层执行器
pub struct HierarchicalExecutor<'a> {
    max_parallel: usize,
    max_failures: usize,
    /// 最大重新规划次数（预留）
    #[allow(dead_code)]
    max_replans: usize,
    /// LLM 后端（用于 Operator 的 ReAct 循环）
    llm_backend: Option<&'a dyn ChatCompletionsBackend>,
    /// Agent 配置
    cfg: Option<AgentConfig>,
    /// HTTP 客户端
    client: Option<std::sync::Arc<reqwest::Client>>,
    /// API 密钥
    api_key: Option<String>,
    /// 工作目录
    working_dir: Option<std::path::PathBuf>,
    /// SSE 发送器
    sse_out: Option<Sender<String>>,
    /// 工具定义列表（用于 Operator 的 LLM 函数调用）
    tools_defs: Vec<Tool>,
    /// Manager Agent（用于失败时重新规划）
    manager: Option<super::manager::ManagerAgent>,
    /// 原始任务（用于失败时重新规划）
    original_task: Option<String>,
    /// 工具审批发送器（用于触发审批对话框）
    tool_approval_out: Option<Sender<String>>,
    /// 工具审批接收器（用于接收用户审批决定）
    tool_approval_rx: Option<Arc<TokioMutex<Receiver<CommandApprovalDecision>>>>,
    /// 分层单轮共享探测缓存：去重 `which` / `--version` 等无副作用探测命令。
    probe_cache: Arc<TokioMutex<super::tool_executor::ProbeCache>>,
    /// 与主 Agent `process_handles` 对齐；缺省在 `with_context` 中填生产表与默认 Docker 后端。
    handler_lookup: Option<crate::tool_registry::HandlerLookupTable>,
    sync_default_sandbox_backend: Option<Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>>,
}

impl HierarchicalExecutor<'_> {
    pub fn new(max_parallel: usize, max_failures: usize) -> Self {
        Self {
            max_parallel,
            max_failures,
            max_replans: 2,
            llm_backend: None,
            cfg: None,
            client: None,
            api_key: None,
            working_dir: None,
            sse_out: None,
            tools_defs: Vec::new(),
            manager: None,
            original_task: None,
            tool_approval_out: None,
            tool_approval_rx: None,
            probe_cache: Arc::new(TokioMutex::new(Default::default())),
            handler_lookup: None,
            sync_default_sandbox_backend: None,
        }
    }
}

impl<'a> HierarchicalExecutor<'a> {
    async fn emit_subgoal_started_timeline(
        &self,
        goal_id: &str,
        goal_description: &str,
        required_tools: &[String],
    ) {
        let Some(ref sse_out) = self.sse_out else {
            return;
        };
        let title = format!("子目标 `{goal_id}`");
        let mut detail = format!(
            "- 阶段：开始执行\n- 目标：{}",
            trim_for_detail(goal_description, 180)
        );
        if !required_tools.is_empty() {
            detail.push_str("\n- 计划工具：");
            detail.push_str(&required_tools.join(", "));
        }
        let payload = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_subgoal_started".to_string(),
                title,
                detail: Some(detail),
            },
        });
        let _ = sse::send_string_logged(sse_out, payload, "hierarchical::subgoal_started_timeline")
            .await;
    }

    async fn emit_assistant_progress_delta_sse(
        &self,
        answer_phase_emitted: &mut bool,
        title: String,
        detail: Option<String>,
    ) {
        let Some(ref sse_out) = self.sse_out else {
            return;
        };
        if !*answer_phase_emitted {
            let phase_payload = sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
                assistant_answer_phase: true,
            });
            let _ = sse::send_string_logged(
                sse_out,
                phase_payload,
                "hierarchical::progress_answer_phase",
            )
            .await;
            *answer_phase_emitted = true;
        }
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let payload = sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_subgoal".to_string(),
                title,
                detail,
            },
        });
        let _ = sse::send_string_logged(sse_out, payload, "hierarchical::progress_timeline").await;
    }

    fn progress_line_for_task_result(result: &TaskResult) -> Option<(String, String)> {
        let status = match &result.status {
            TaskStatus::Completed => "完成",
            TaskStatus::Failed { .. } => "失败",
            TaskStatus::Skipped { .. } => "跳过",
            TaskStatus::NeedsDecomposition { .. } => "需分解",
            TaskStatus::Pending | TaskStatus::InProgress => return None,
        };
        let title = format!("子目标 `{}`", result.task_id);

        let mut details = Vec::new();
        details.push("- 阶段：执行完成".to_string());
        details.push(format!("- 结果：{status}"));
        let tools = if result.tools_invoked.is_empty() {
            "无".to_string()
        } else {
            let mut seen = std::collections::BTreeSet::new();
            for t in &result.tools_invoked {
                seen.insert(t.as_str());
            }
            seen.into_iter().take(5).collect::<Vec<_>>().join(", ")
        };
        details.push(format!("- 工具：{tools}"));
        details.push(format!(
            "- 证据：{}",
            summarize_subgoal_evidence(result).unwrap_or_else(|| "无额外证据".to_string())
        ));

        if let TaskStatus::Failed { reason } = &result.status {
            details.push(format!("- 失败原因：{}", trim_for_detail(reason, 140)));
        }
        if let TaskStatus::Skipped { reason } = &result.status {
            details.push(format!("- 跳过原因：{}", trim_for_detail(reason, 140)));
        }
        if let TaskStatus::NeedsDecomposition {
            reason,
            suggested_subgoals,
        } = &result.status
        {
            details.push(format!(
                "- 分解建议：{}（建议子目标数={})",
                trim_for_detail(reason, 120),
                suggested_subgoals
            ));
        }

        Some((title, details.join("\n")))
    }

    /// 设置执行上下文
    pub fn with_context(
        mut self,
        llm_backend: &'a dyn ChatCompletionsBackend,
        cfg: AgentConfig,
        client: std::sync::Arc<reqwest::Client>,
        api_key: String,
        working_dir: std::path::PathBuf,
    ) -> Self {
        self.llm_backend = Some(llm_backend);
        self.cfg = Some(cfg);
        self.client = Some(client);
        self.api_key = Some(api_key);
        self.working_dir = Some(working_dir);
        self.handler_lookup = Some(crate::tool_registry::HandlerLookupTable::default_dispatch());
        self.sync_default_sandbox_backend =
            Some(crate::tool_sandbox::default_sync_default_sandbox_backend());
        self
    }

    /// 覆盖默认的工具分发表与 Docker 沙盒后端（与 [`crate::process_handles::ProcessHandles`] 同源）。
    pub fn with_process_tool_handles(
        mut self,
        handler_lookup: crate::tool_registry::HandlerLookupTable,
        sync_default_sandbox_backend: Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    ) -> Self {
        self.handler_lookup = Some(handler_lookup);
        self.sync_default_sandbox_backend = Some(sync_default_sandbox_backend);
        self
    }

    /// 设置 SSE 发送器
    pub fn with_sse(mut self, sse_out: Sender<String>) -> Self {
        self.sse_out = Some(sse_out);
        self
    }

    /// 设置工具定义列表
    pub fn with_tools_defs(mut self, tools_defs: Vec<Tool>) -> Self {
        self.tools_defs = tools_defs;
        self
    }

    /// 设置 Manager Agent（用于失败时重新规划）
    pub fn with_manager(mut self, manager: super::manager::ManagerAgent) -> Self {
        self.manager = Some(manager);
        self
    }

    /// 设置原始任务（用于失败时重新规划）
    pub fn with_original_task(mut self, task: String) -> Self {
        self.original_task = Some(task);
        self
    }

    /// 设置工具审批上下文（用于敏感操作的交互式审批）
    pub fn with_tool_approval(
        mut self,
        out_tx: Sender<String>,
        approval_rx: Arc<TokioMutex<Receiver<CommandApprovalDecision>>>,
    ) -> Self {
        self.tool_approval_out = Some(out_tx);
        self.tool_approval_rx = Some(approval_rx);
        self
    }
}

/// 子目标顺序/并行执行、验证重试与 `BuildState` 更新（原 `execution_body.inc.rs`，现为独立模块以便导航）。
#[path = "execution_impl.rs"]
mod execution_impl;

/// `execute_with_result` 实现拆至此模块以降低圈复杂度。
#[path = "execution_with_result.rs"]
mod execution_with_result;

/// 单子目标 `execute_single`（验证/反思/Manager）拆分。
#[path = "execution_execute_single.rs"]
mod execution_execute_single;

/// `TaskResult` → `BuildState` 增量更新。
#[path = "execution_build_state_apply.rs"]
mod execution_build_state_apply;

/// `try_replan` 预留实现。
#[path = "execution_try_replan.rs"]
mod execution_try_replan;
