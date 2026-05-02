//! 分层执行器：按依赖层级执行子目标

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::backend::{ChatCompletionsBackend, OPENAI_COMPAT_BACKEND};
use crate::sse;

use super::artifact_resolver::ArtifactResolver;
use super::artifact_store::ArtifactStore;
use super::build_state::BuildState;
use super::events;
use super::execution_helpers::{
    Dag, summarize_subgoal_evidence, supplement_subgoal_required_tools, trim_for_detail,
    truncate_goal_desc,
};
use super::goal_verifier::{GoalVerifier, VerificationResult};
use super::manager::{ManagerLlmContext, ManagerOutput, ReflectAndReplanContext, handle_failure};
use super::operator::{OperatorAgent, OperatorConfig};
use super::task::{
    ArtifactKind, BuildArtifactKind, ExecutionStrategy, SubGoal, TaskResult, TaskStatus,
};
use super::tool_executor::{ToolExecutor, ToolExecutorContext};
use crate::types::{CommandApprovalDecision, Tool};
use log::{error, info, warn};
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

    /// 执行并返回详细结果
    pub async fn execute_with_result(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<HierarchicalExecutionResult, ExecutionError> {
        let start_time = Instant::now();
        let sub_goals = manager_output.sub_goals;
        let strategy = manager_output.execution_strategy;
        let goal_expected_outputs: HashMap<String, Vec<String>> = sub_goals
            .iter()
            .map(|g| {
                let hints = g
                    .acceptance
                    .as_ref()
                    .map(|a| {
                        a.expect_output_contains
                            .iter()
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (g.goal_id.clone(), hints)
            })
            .collect();

        info!(
            target: "crabmate",
            "Hierarchical execution started: {} goals, strategy={:?}",
            sub_goals.len(),
            strategy
        );

        // 发射 SSE 事件：分层执行开始
        if let Some(ref sse_out) = self.sse_out {
            let trace = events::build_hierarchical_started_trace(
                sub_goals.len(),
                &format!("{:?}", strategy),
            );
            let _ = sse::send_string_logged(
                sse_out,
                sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                "hierarchical::started",
            )
            .await;
        }

        if sub_goals.is_empty() {
            return Ok(HierarchicalExecutionResult {
                results: Vec::new(),
                total_duration_ms: start_time.elapsed().as_millis() as u64,
                total_completed: 0,
                total_failed: 0,
                goal_expected_outputs: HashMap::new(),
            });
        }

        // 构建 DAG
        let dag = Dag::build(&sub_goals)?;

        // 计算拓扑层级
        let levels = dag.topological_levels()?;

        info!(
            target: "crabmate",
            "Hierarchical execution: {} goals in {} levels",
            sub_goals.len(),
            levels.len()
        );

        let mut artifact_store = ArtifactStore::new();
        // 尝试从磁盘加载之前的构建状态（用于增量编译）
        let mut build_state = if let Some(ref working_dir) = self.working_dir {
            BuildState::load_or_create(working_dir)
        } else {
            BuildState::default()
        };
        info!(
            target: "crabmate",
            "Loaded build state: {} source files tracked, {} artifacts cached",
            build_state.source_files.len(),
            build_state.artifact_cache.len()
        );
        let mut all_results: Vec<TaskResult> = Vec::new();
        let mut answer_phase_emitted = false;

        // 按层级执行
        for (level_idx, level) in levels.iter().enumerate() {
            info!(target: "crabmate", "Executing level {} with {} goals", level_idx, level.len());

            // 发射 SSE 事件：层级开始
            if let Some(ref sse_out) = self.sse_out {
                let trace = events::build_level_started_trace(level_idx, level);
                let _ = sse::send_string_logged(
                    sse_out,
                    sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                    "hierarchical::level_started",
                )
                .await;
            }

            // 获取该层级的子目标
            let level_goals: Vec<_> = level
                .iter()
                .filter_map(|id| sub_goals.iter().find(|g| &g.goal_id == id))
                .collect();

            // 按策略执行（传递 artifact_store 和 build_state；`all_results` 供同层/后续层补全 I/O 契约与步摘要）
            let level_results = match strategy {
                ExecutionStrategy::Sequential => {
                    self.execute_sequential(
                        &level_goals,
                        &all_results,
                        &mut artifact_store,
                        &mut build_state,
                    )
                    .await
                }
                ExecutionStrategy::Parallel | ExecutionStrategy::Hybrid => {
                    self.execute_parallel(
                        &level_goals,
                        &all_results,
                        &mut artifact_store,
                        &mut build_state,
                    )
                    .await
                }
            }?;

            // 更新 artifact store 和 build_state
            for result in &level_results {
                if matches!(result.status, TaskStatus::Completed) {
                    artifact_store.store_result(result);
                    // 从结果中提取构建产物并更新 build_state
                    self.update_build_state_from_result(&mut build_state, result);
                }
                all_results.push(result.clone());
                if let Some((title, detail)) = Self::progress_line_for_task_result(result) {
                    self.emit_assistant_progress_delta_sse(
                        &mut answer_phase_emitted,
                        title,
                        Some(detail),
                    )
                    .await;
                }
            }

            // 发射 SSE 事件：层级完成
            if let Some(ref sse_out) = self.sse_out {
                let level_completed = level_results
                    .iter()
                    .filter(|r| matches!(r.status, TaskStatus::Completed))
                    .count();
                let level_failed = level_results
                    .iter()
                    .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
                    .count();
                let trace =
                    events::build_level_finished_trace(level_idx, level_completed, level_failed);
                let _ = sse::send_string_logged(
                    sse_out,
                    sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                    "hierarchical::level_finished",
                )
                .await;
            }

            // 检查失败
            let (_, failed, decision) = handle_failure(&all_results, self.max_failures);

            // 如果有失败，记录可供重新规划的上下文信息
            if !failed.is_empty() {
                let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] {} failures at level {}. Artifacts available for replan: {}, original_task: {}",
                    failed.len(),
                    level_idx,
                    artifacts.len(),
                    self.original_task.as_deref().unwrap_or("N/A")
                );
                // 失败时的重新规划逻辑已准备好，当 Manager.replan_with_artifacts 被调用时会使用这些信息
            }

            if !failed.is_empty()
                && let super::manager::FailureDecision::Abort { .. } = decision
            {
                error!(
                    target: "crabmate",
                    "Max failures reached at level {}, aborting",
                    level_idx
                );
                return Err(ExecutionError::MaxFailuresReached(format!(
                    "Failed {} goals, exceeding threshold",
                    failed.len()
                )));
            }
        }

        let total_duration_ms = start_time.elapsed().as_millis() as u64;
        let total_completed = all_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Completed))
            .count();
        let total_failed = all_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
            .count();

        info!(
            target: "crabmate",
            "Hierarchical execution finished: {} completed, {} failed, {}ms",
            total_completed,
            total_failed,
            total_duration_ms
        );

        // 保存 BuildState 到磁盘（用于增量编译）
        if let Some(ref working_dir) = self.working_dir
            && let Err(e) = build_state.save_to_disk(working_dir)
        {
            warn!(
                target: "crabmate",
                "Failed to save build state: {}",
                e
            );
        }

        // 发射 SSE 事件：分层执行完成
        if let Some(ref sse_out) = self.sse_out {
            let trace = events::build_hierarchical_finished_trace(
                total_completed,
                total_failed,
                total_duration_ms,
            );
            let _ = sse::send_string_logged(
                sse_out,
                sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                "hierarchical::finished",
            )
            .await;
        }

        Ok(HierarchicalExecutionResult {
            results: all_results,
            total_duration_ms,
            total_completed,
            total_failed,
            goal_expected_outputs,
        })
    }
}

include!("execution_body.inc.rs");
