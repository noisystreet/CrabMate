//! 分层执行器：按依赖层级执行子目标

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::sse;

use super::artifact_resolver::ArtifactResolver;
use super::artifact_store::ArtifactStore;
use super::build_state::BuildState;
use super::events;
use super::manager::{ManagerOutput, handle_failure};
use super::operator::{OperatorAgent, OperatorConfig};
use super::task::{
    Artifact, ArtifactKind, BuildArtifactKind, ExecutionStrategy, SubGoal, TaskResult, TaskStatus,
};
use super::tool_executor::ToolExecutor;
use crate::types::Tool;
use log::{error, info, warn};

/// 分层执行结果
#[derive(Debug, Clone)]
pub struct HierarchicalExecutionResult {
    pub results: Vec<TaskResult>,
    pub total_duration_ms: u64,
    pub total_completed: usize,
    pub total_failed: usize,
}

/// 分层执行器错误
#[derive(Debug)]
pub enum ExecutionError {
    DagError(String),
    MaxFailuresReached(String),
    OperatorError(super::operator::OperatorError),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::DagError(s) => write!(f, "DAG error: {}", s),
            ExecutionError::MaxFailuresReached(s) => write!(f, "Max failures: {}", s),
            ExecutionError::OperatorError(e) => write!(f, "Operator error: {}", e),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<super::operator::OperatorError> for ExecutionError {
    fn from(e: super::operator::OperatorError) -> Self {
        ExecutionError::OperatorError(e)
    }
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
    /// 原始任务（用于重新规划）
    original_task: Option<String>,
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
        }
    }
}

impl<'a> HierarchicalExecutor<'a> {
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

    /// 执行并返回详细结果
    pub async fn execute_with_result(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<HierarchicalExecutionResult, ExecutionError> {
        let start_time = Instant::now();
        let sub_goals = manager_output.sub_goals;
        let strategy = manager_output.execution_strategy;

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
        let mut all_results = Vec::new();

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

            // 按策略执行（传递 artifact_store 和 build_state）
            let level_results = match strategy {
                ExecutionStrategy::Sequential => {
                    self.execute_sequential(&level_goals, &mut artifact_store, &mut build_state)
                        .await
                }
                ExecutionStrategy::Parallel | ExecutionStrategy::Hybrid => {
                    self.execute_parallel(&level_goals, &mut artifact_store, &mut build_state)
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
        })
    }

    /// 执行子目标列表（保持原有接口兼容）
    pub async fn execute(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let result = self.execute_with_result(manager_output).await?;
        Ok(result.results)
    }

    /// 顺序执行
    async fn execute_sequential(
        &self,
        goals: &[&SubGoal],
        artifact_store: &mut ArtifactStore,
        build_state: &mut BuildState,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let mut results = Vec::new();

        for goal in goals {
            let result = self
                .execute_single(goal, artifact_store, build_state)
                .await?;
            results.push(result);
        }

        Ok(results)
    }

    /// 并行执行
    async fn execute_parallel(
        &self,
        goals: &[&SubGoal],
        _artifact_store: &mut ArtifactStore,
        build_state: &mut BuildState,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let mut results = Vec::new();

        // 分块执行（简化版：顺序执行）
        for chunk in goals.chunks(self.max_parallel) {
            for goal in chunk {
                let mut store = ArtifactStore::new();
                let result = self.execute_single(goal, &mut store, build_state).await?;
                results.push(result);
            }
        }

        Ok(results)
    }

    /// 执行单个子目标（带重试循环）
    async fn execute_single(
        &self,
        goal: &SubGoal,
        artifact_store: &mut ArtifactStore,
        build_state: &BuildState,
    ) -> Result<TaskResult, ExecutionError> {
        let mut current_goal = goal.clone();
        const MAX_RETRIES: usize = 3;

        for attempt in 0..MAX_RETRIES {
            let result = self
                .execute_single_impl(&current_goal, artifact_store, build_state)
                .await?;

            // 如果成功或不是 Manager 处理范围内的失败，直接返回
            if !matches!(result.status, TaskStatus::Failed { .. }) {
                return Ok(result);
            }

            // Analyze 类型的子目标：失败后直接跳过，不重试
            if matches!(current_goal.goal_type, super::task::GoalType::Analyze) {
                info!(target: "crabmate", "[HIERARCHICAL] Executor: Analyze type goal failed, skipping directly: {}", current_goal.goal_id);
                return Ok(TaskResult {
                    task_id: current_goal.goal_id.clone(),
                    status: TaskStatus::Skipped {
                        reason: result
                            .error
                            .clone()
                            .unwrap_or_else(|| "Analyze goal failed".to_string()),
                    },
                    output: result.output,
                    error: result.error,
                    artifacts: result.artifacts,
                    duration_ms: result.duration_ms,
                });
            }

            // 尝试获取 Manager 的决策
            let decision = if let Some(ref manager) = self.manager {
                let error_msg = result.error.as_deref().unwrap_or("Unknown error");
                let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
                match self
                    .ask_manager_for_decision(manager, &current_goal, error_msg, &artifacts)
                    .await
                {
                    Some(d) => d,
                    None => return Ok(result), // Manager 不可用，返回失败结果
                }
            } else {
                return Ok(result); // 没有 Manager，返回失败结果
            };

            match decision {
                super::manager::ManagerDecision::Retry { updated_goal } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to retry (attempt {}/{})", attempt + 1, MAX_RETRIES);
                    current_goal = updated_goal;
                    continue;
                }
                super::manager::ManagerDecision::Skip { reason } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to skip: {}", reason);
                    return Ok(TaskResult {
                        task_id: current_goal.goal_id.clone(),
                        status: TaskStatus::Skipped { reason },
                        output: result.output,
                        error: result.error,
                        artifacts: result.artifacts,
                        duration_ms: result.duration_ms,
                    });
                }
                super::manager::ManagerDecision::Abort { reason } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to abort: {}", reason);
                    return Err(ExecutionError::MaxFailuresReached(reason));
                }
            }
        }

        // 达到最大重试次数，返回最后一次失败结果
        info!(target: "crabmate", "[HIERARCHICAL] Executor: max retries ({}) reached for goal_id={}", MAX_RETRIES, current_goal.goal_id);
        Ok(TaskResult {
            task_id: current_goal.goal_id.clone(),
            status: TaskStatus::Failed {
                reason: format!("Max retries ({}) reached", MAX_RETRIES),
            },
            output: None,
            error: Some(format!("Max retries ({}) reached", MAX_RETRIES)),
            artifacts: Vec::new(),
            duration_ms: 0,
        })
    }

    /// 执行单个子目标的实际逻辑
    async fn execute_single_impl(
        &self,
        goal: &SubGoal,
        artifact_store: &mut ArtifactStore,
        build_state: &BuildState,
    ) -> Result<TaskResult, ExecutionError> {
        // 获取依赖的 artifacts
        let deps = artifact_store.get_dependencies(&goal.depends_on);

        info!(
            target: "crabmate",
            "[HIERARCHICAL] Executor: executing goal_id={} desc={} deps={} tools={:?}",
            goal.goal_id,
            truncate_goal_desc(&goal.description),
            deps.len(),
            goal.required_tools
        );

        // 发射 SSE 事件：子目标开始
        if let Some(ref sse_out) = self.sse_out {
            let trace = events::build_subgoal_started_trace(&goal.goal_id, &goal.description);
            let _ = sse::send_string_logged(
                sse_out,
                sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                "hierarchical::subgoal_started",
            )
            .await;
        }

        // 构建 Operator 配置
        let allowed_tools = goal.required_tools.clone();
        info!(
            target: "crabmate",
            "[HIERARCHICAL] Operator: allowed_tools={:?}",
            allowed_tools
        );
        // 仅将 allowed_tools 交集内的工具定义传给 LLM（避免 LLM 看到不可用工具）
        let tools_defs_for_llm = if allowed_tools.is_empty() {
            self.tools_defs.clone()
        } else {
            self.tools_defs
                .iter()
                .filter(|t| allowed_tools.contains(&t.function.name))
                .cloned()
                .collect()
        };
        // 创建产物解析器，用于注入构建产物路径
        let _resolver = ArtifactResolver::new(artifact_store, Some(build_state));

        let op_config = OperatorConfig {
            max_iterations: 10,
            allowed_tools: allowed_tools.clone(),
            tools_defs: tools_defs_for_llm.clone(),
            sse_out: self.sse_out.clone(),
            artifact_store: Some(artifact_store.clone()),
            build_state: Some(Arc::new(Mutex::new(build_state.clone()))),
        };
        log::info!(target: "crabmate", "[HIERARCHICAL] execute_single: sse_out is {:?}, tools_defs count={}", self.sse_out.is_some(), tools_defs_for_llm.len());

        let operator = OperatorAgent::new(op_config);

        // 根据是否有完整上下文选择执行方法
        let result =
            if let (Some(llm_backend), Some(cfg), Some(client), Some(api_key), Some(work_dir)) = (
                self.llm_backend,
                self.cfg.as_ref(),
                self.client.as_ref(),
                self.api_key.as_ref(),
                self.working_dir.as_ref(),
            ) {
                // 有完整上下文，使用带工具的执行
                let tool_executor_ctx = super::tool_executor::ToolExecutorContext::new(
                    Arc::new(cfg.clone()),
                    work_dir.clone(),
                );
                let tool_executor = ToolExecutor::new(tool_executor_ctx);
                // 构建额外上下文（依赖 artifacts）
                let extra_context = if deps.is_empty() {
                    None
                } else {
                    Some(self.format_dependencies_context(&deps))
                };
                operator
                    .execute_with_tools(
                        goal,
                        cfg,
                        llm_backend,
                        client,
                        api_key,
                        &tool_executor,
                        extra_context.as_deref(),
                    )
                    .await
            } else {
                // 降级使用简化版本
                operator.execute(goal).await
            };

        let result = result.map_err(ExecutionError::OperatorError)?;

        // 发射 SSE 事件：子目标完成
        if let Some(ref sse_out) = self.sse_out {
            let status_str = match &result.status {
                TaskStatus::Completed => "completed",
                TaskStatus::Failed { .. } => "failed",
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Skipped { .. } => "skipped",
            };
            let trace =
                events::build_subgoal_finished_trace(&goal.goal_id, status_str, result.duration_ms);
            let _ = sse::send_string_logged(
                sse_out,
                sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                "hierarchical::subgoal_finished",
            )
            .await;
        }

        // 如果完成，存储 artifacts
        if matches!(result.status, super::task::TaskStatus::Completed) {
            artifact_store.store_result(&result);
        }

        Ok(result)
    }

    /// 询问 Manager 如何处理失败的子目标
    async fn ask_manager_for_decision(
        &self,
        manager: &super::manager::ManagerAgent,
        failed_goal: &SubGoal,
        error_message: &str,
        previous_artifacts: &[super::task::Artifact],
    ) -> Option<super::manager::ManagerDecision> {
        // 提前提取所有需要的数据，避免生命周期问题
        let manager = manager.clone();
        let goal = failed_goal.clone();
        let error = error_message.to_string();
        let artifacts = previous_artifacts.to_vec();

        let cfg = self.cfg.clone()?;
        let llm_backend = self.llm_backend?;
        let client = self.client.as_ref()?.clone();
        let api_key = self.api_key.as_ref()?.clone();
        let working_dir = self.working_dir.as_ref()?.clone();
        let tools_defs = self.tools_defs.clone();

        // 调用 Manager
        match manager
            .handle_failed_goal(
                &goal,
                &error,
                &cfg,
                llm_backend,
                &client,
                &api_key,
                &working_dir,
                &tools_defs,
                &artifacts,
            )
            .await
        {
            Ok(decision) => Some(decision),
            Err(e) => {
                log::warn!(target: "crabmate", "[HIERARCHICAL] ask_manager_for_decision failed: {}", e);
                None
            }
        }
    }

    /// 尝试基于已完成的结果和产物重新规划（预留接口）
    #[allow(dead_code)]
    async fn try_replan(
        &self,
        previous_results: &[TaskResult],
        artifact_store: &ArtifactStore,
    ) -> Result<Vec<SubGoal>, ExecutionError> {
        let manager = self.manager.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached("No manager available for replanning".to_string())
        })?;

        let original_task = self.original_task.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached(
                "No original task available for replanning".to_string(),
            )
        })?;

        let working_dir = self.working_dir.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached(
                "No working_dir available for replanning".to_string(),
            )
        })?;

        let cfg = self.cfg.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached("No cfg available for replanning".to_string())
        })?;

        let llm_backend = self.llm_backend.ok_or_else(|| {
            ExecutionError::MaxFailuresReached(
                "No llm_backend available for replanning".to_string(),
            )
        })?;

        let client = self.client.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached("No client available for replanning".to_string())
        })?;

        let api_key = self.api_key.as_ref().ok_or_else(|| {
            ExecutionError::MaxFailuresReached("No api_key available for replanning".to_string())
        })?;

        let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();

        let manager_output = manager
            .replan_with_artifacts(
                original_task,
                cfg,
                llm_backend,
                client,
                api_key,
                working_dir,
                &self.tools_defs,
                previous_results,
                &artifacts,
            )
            .await
            .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

        Ok(manager_output.sub_goals)
    }

    /// 格式化依赖产物上下文
    fn format_dependencies_context(&self, deps: &[&Artifact]) -> String {
        if deps.is_empty() {
            return String::new();
        }

        let mut lines = Vec::new();
        for dep in deps {
            let kind_str = format!("{:?}", dep.kind).to_lowercase();
            if let Some(ref path) = dep.path {
                lines.push(format!("- [{}] {}: 路径={}", kind_str, dep.name, path));
            } else if let Some(ref content) = dep.content {
                // 如果是文件内容，只显示前 200 字符
                let preview = if content.len() > 200 {
                    format!("{}... ({} chars)", &content[..200], content.len())
                } else {
                    content.clone()
                };
                lines.push(format!("- [{}] {}:\n{}", kind_str, dep.name, preview));
            } else {
                lines.push(format!("- [{}] {}", kind_str, dep.name));
            }
        }
        lines.join("\n")
    }

    /// 从执行结果中更新构建状态
    fn update_build_state_from_result(&self, build_state: &mut BuildState, result: &TaskResult) {
        for artifact in &result.artifacts {
            // 根据产物类型更新 build_state
            match &artifact.kind {
                ArtifactKind::BuildArtifact(build_kind) => {
                    if let Some(ref path) = artifact.path {
                        let path_buf = std::path::PathBuf::from(path);
                        match build_kind {
                            BuildArtifactKind::SourceFile => {
                                // 源文件：记录内容和哈希
                                if let Some(ref content) = artifact.content {
                                    build_state.record_source_file(&path_buf, content);
                                }
                            }
                            BuildArtifactKind::ObjectFile => {
                                build_state.add_object_file(path_buf);
                            }
                            BuildArtifactKind::Executable => {
                                build_state.add_executable(path_buf);
                            }
                            BuildArtifactKind::StaticLibrary => {
                                build_state.add_static_library(path_buf);
                            }
                            BuildArtifactKind::DynamicLibrary => {
                                build_state.add_dynamic_library(path_buf);
                            }
                            BuildArtifactKind::BuildLog => {
                                // 构建日志：暂不解析命令
                            }
                        }
                    }
                }
                ArtifactKind::File => {
                    // 检查是否是源码文件
                    if let Some(ref path) = artifact.path {
                        let path_buf = std::path::PathBuf::from(path);
                        if let Some(ext) = path_buf.extension() {
                            let ext = ext.to_string_lossy().to_lowercase();
                            if matches!(ext.as_str(), "c" | "cpp" | "cc" | "h" | "hpp")
                                && let Some(ref content) = artifact.content
                            {
                                build_state.record_source_file(&path_buf, content);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // 尝试从输出中提取构建目录
        if let Some(ref output) = result.output {
            // 检查常见的构建目录模式
            for line in output.lines() {
                if (line.contains("build/") || line.contains("Build directory:"))
                    && let Some(idx) = line.find("build")
                {
                    let build_dir = &line[idx..].split_whitespace().next().unwrap_or("build");
                    let build_path = std::path::PathBuf::from(build_dir);
                    if build_path.exists() {
                        build_state.set_build_dir(build_path);
                        break;
                    }
                }
            }
        }
    }
}

/// 截断目标描述用于日志（按字符边界截断，支持中文）
fn truncate_goal_desc(desc: &str) -> String {
    const MAX_LEN: usize = 80;
    if desc.len() > MAX_LEN {
        let truncated = desc
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &desc[..truncated])
    } else {
        desc.to_string()
    }
}

/// DAG 构建器
#[derive(Debug)]
struct Dag {
    nodes: HashSet<String>,
    edges: HashMap<String, HashSet<String>>,
}

impl Dag {
    fn build(goals: &[SubGoal]) -> Result<Self, ExecutionError> {
        let mut dag = Dag {
            nodes: HashSet::new(),
            edges: HashMap::new(),
        };

        for goal in goals {
            dag.nodes.insert(goal.goal_id.clone());
            dag.edges.entry(goal.goal_id.clone()).or_default();

            for dep in &goal.depends_on {
                if !dag.nodes.contains(dep) {
                    dag.nodes.insert(dep.clone());
                    dag.edges.entry(dep.clone()).or_default();
                }
                dag.edges.get_mut(dep).unwrap().insert(goal.goal_id.clone());
            }
        }

        Ok(dag)
    }

    /// 计算拓扑层级
    fn topological_levels(&self) -> Result<Vec<Vec<String>>, ExecutionError> {
        let mut levels = Vec::new();
        let mut remaining = self.nodes.clone();
        let mut in_degree: HashMap<String, usize> =
            self.nodes.iter().map(|id| (id.clone(), 0)).collect();

        // 计算入度
        for targets in self.edges.values() {
            for target in targets {
                if let Some(d) = in_degree.get_mut(target) {
                    *d += 1;
                }
            }
        }

        while !remaining.is_empty() {
            // 找到入度为 0 的节点
            let level: Vec<String> = remaining
                .iter()
                .filter(|id| in_degree.get(*id) == Some(&0))
                .cloned()
                .collect();

            if level.is_empty() {
                return Err(ExecutionError::DagError(
                    "Cycle detected in dependencies".to_string(),
                ));
            }

            for id in &level {
                remaining.remove(id);
                if let Some(targets) = self.edges.get(id) {
                    for target in targets {
                        if let Some(d) = in_degree.get_mut(target) {
                            *d -= 1;
                        }
                    }
                }
            }

            levels.push(level);
        }

        Ok(levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_levels() {
        let goals = vec![
            SubGoal::new("a", "task a"),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("c", "task c").with_depends_on(vec!["a".to_string()]),
            SubGoal::new("d", "task d").with_depends_on(vec!["b".to_string(), "c".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let levels = dag.topological_levels().unwrap();

        assert_eq!(levels.len(), 3);
        assert!(levels[0].contains(&"a".to_string()));
        assert!(levels[1].contains(&"b".to_string()) || levels[1].contains(&"c".to_string()));
    }

    #[test]
    fn test_dag_cycle_detection() {
        let goals = vec![
            SubGoal::new("a", "task a").with_depends_on(vec!["b".to_string()]),
            SubGoal::new("b", "task b").with_depends_on(vec!["a".to_string()]),
        ];

        let dag = Dag::build(&goals).unwrap();
        let result = dag.topological_levels();

        assert!(result.is_err());
    }
}
