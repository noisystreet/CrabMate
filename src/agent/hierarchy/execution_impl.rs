//! 子目标顺序/并行执行、单步验证/反思与 `BuildState` 更新（自 `include!` 迁出，便于在仓库内搜索与分层导航）。
//! 由 [`super`]（`execution.rs`）中的 **`execute_with_result`** 调用；本文件仅扩展同一 `HierarchicalExecutor` 的 `impl` 块。

use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

use super::super::artifact_resolver::ArtifactResolver;
use super::super::artifact_store::ArtifactStore;
use super::super::build_state::BuildState;
use super::super::events;
use super::super::execution_error::ExecutionError;
use super::super::execution_helpers::truncate_goal_desc;
use super::super::goal_verifier::{GoalVerifier, VerificationResult};
use super::super::manager::{ManagerLlmContext, ManagerOutput, ReflectAndReplanContext};
use super::super::operator::{OperatorAgent, OperatorConfig};
use super::super::supplement_subgoal_required_tools;
use super::super::task::{
    ArtifactKind, BuildArtifactKind, GoalType, SubGoal, TaskResult, TaskStatus,
};
use super::super::tool_executor::ToolExecutor;
use crate::sse;
use log::{info, warn};

mod execution_parallel;

impl<'a> super::HierarchicalExecutor<'a> {
    /// 执行子目标列表（保持原有接口兼容）
    pub async fn execute(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let result = self.execute_with_result(manager_output).await?;
        Ok(result.results)
    }

    /// 顺序执行
    pub(super) async fn execute_sequential(
        &self,
        goals: &[&SubGoal],
        prior_subgoal_results: &[TaskResult],
        artifact_store: &mut ArtifactStore,
        build_state: &mut BuildState,
    ) -> Result<Vec<TaskResult>, ExecutionError> {
        let current_level_ids: std::collections::HashSet<String> =
            goals.iter().map(|g| g.goal_id.to_string()).collect();
        let mut acc: Vec<TaskResult> = prior_subgoal_results.to_vec();
        let mut results = Vec::new();

        for goal in goals {
            let result = self
                .execute_single(goal, &acc, &current_level_ids, artifact_store, build_state)
                .await?;
            acc.push(result.clone());
            results.push(result);
        }

        Ok(results)
    }

    /// 执行单个子目标（带验证和重试循环）
    ///
    /// 执行流程：执行 → 验证 → （失败时）反思/重试
    async fn execute_single(
        &self,
        goal: &SubGoal,
        prior_subgoals_for_context: &[TaskResult],
        current_level_goal_ids: &HashSet<String>,
        artifact_store: &mut ArtifactStore,
        build_state: &BuildState,
    ) -> Result<TaskResult, ExecutionError> {
        if let Some(msg) =
            super::super::subgoal_context::validate_depends_consumes_consistency(goal)
        {
            warn!(target: "crabmate", "[HIERARCHICAL] I/O 契约: {}", msg);
        }

        let mut current_goal = super::super::subgoal_context::ensure_consumes_from_dependencies(
            goal,
            prior_subgoals_for_context,
            current_level_goal_ids,
            true,
        );
        super::super::subgoal_context::normalize_subgoal_io_contracts(&mut current_goal);
        let max_retries = current_goal.max_retries.unwrap_or(3);

        // 创建验证器
        let verifier = self
            .working_dir
            .as_ref()
            .map(|dir| GoalVerifier::new(dir.clone()));

        for attempt in 0..max_retries {
            // 阶段 1: 执行
            let result = self
                .execute_single_impl(
                    &current_goal,
                    prior_subgoals_for_context,
                    artifact_store,
                    build_state,
                )
                .await?;

            // 检查是否需要动态分解
            if let TaskStatus::NeedsDecomposition {
                reason,
                suggested_subgoals,
            } = &result.status
            {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Goal {} needs decomposition (suggested {} subgoals): {}",
                    current_goal.goal_id,
                    suggested_subgoals,
                    reason
                );

                // 尝试通过 Manager 进行反思和重新规划
                // NeedsDecomposition 不是失败，保留原始状态进入反思流程
                if let Some(ref _manager) = self.manager {
                    let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
                    let reflection_result = self
                        .reflect_and_replan(
                            _manager,
                            &current_goal,
                            &format!("任务过于复杂，建议分解为 {} 个子目标", suggested_subgoals),
                            &result,
                            &artifacts,
                        )
                        .await;

                    match reflection_result {
                        Some(updated_goal) => {
                            info!(
                                target: "crabmate",
                                "[HIERARCHICAL] Executor: Manager replanned goal {} for decomposition",
                                current_goal.goal_id
                            );
                            current_goal = updated_goal;
                            continue;
                        }
                        None => {
                            // 反思失败，返回需要分解的状态
                            return Ok(result);
                        }
                    }
                }

                // 无法分解，返回需要分解的状态
                return Ok(result);
            }

            // 阶段 2: 验证（如果有定义验收条件）
            if let Some(ref v) = verifier {
                let verify_result = v.verify(&current_goal, &result);

                // 发射 SSE 事件：验证结果
                if let Some(ref sse_out) = self.sse_out {
                    let trace = match &verify_result {
                        VerificationResult::Pass => {
                            events::build_verification_passed_trace(&current_goal.goal_id)
                        }
                        VerificationResult::Fail { reason } => {
                            events::build_verification_failed_trace(&current_goal.goal_id, reason)
                        }
                        VerificationResult::EscalateHuman { reason } => {
                            events::build_verification_escalated_trace(
                                &current_goal.goal_id,
                                reason,
                            )
                        }
                    };
                    let _ = sse::send_string_logged(
                        sse_out,
                        sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                        "hierarchical::verification",
                    )
                    .await;
                }

                match verify_result {
                    VerificationResult::Pass => {
                        // 验证通过，返回结果
                        info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Executor: Goal {} verification passed",
                            current_goal.goal_id
                        );
                        return Ok(result);
                    }
                    VerificationResult::Fail { reason } => {
                        // 验证失败，需要反思和重试
                        warn!(
                            target: "crabmate",
                            "[HIERARCHICAL] Executor: Goal {} verification failed: {}. Attempt {}/{}",
                            current_goal.goal_id,
                            reason,
                            attempt + 1,
                            max_retries
                        );

                        // 如果还有重试次数，尝试反思和修复
                        if attempt < max_retries - 1
                            && let Some(ref manager) = self.manager
                        {
                            let artifacts: Vec<_> =
                                artifact_store.all().into_iter().cloned().collect();
                            let reflection_result = self
                                .reflect_and_replan(
                                    manager,
                                    &current_goal,
                                    &reason,
                                    &result,
                                    &artifacts,
                                )
                                .await;

                            match reflection_result {
                                Some(updated_goal) => {
                                    info!(
                                        target: "crabmate",
                                        "[HIERARCHICAL] Executor: Replanning goal {}",
                                        current_goal.goal_id
                                    );
                                    current_goal = updated_goal;
                                    continue;
                                }
                                None => {
                                    // 反思失败，返回验证失败的结果
                                    return Ok(TaskResult {
                                        task_id: current_goal.goal_id.clone(),
                                        status: TaskStatus::Failed {
                                            reason: format!("Verification failed: {}", reason),
                                        },
                                        output: result.output,
                                        error: Some(format!("Verification failed: {}", reason)),
                                        artifacts: result.artifacts,
                                        duration_ms: result.duration_ms,
                                        tools_invoked: result.tools_invoked.clone(),
                                    });
                                }
                            }
                        }

                        // 没有 Manager 或达到最大重试次数，返回验证失败
                        return Ok(TaskResult {
                            task_id: current_goal.goal_id.clone(),
                            status: TaskStatus::Failed {
                                reason: format!("Verification failed: {}", reason),
                            },
                            output: result.output,
                            error: Some(format!("Verification failed: {}", reason)),
                            artifacts: result.artifacts,
                            duration_ms: result.duration_ms,
                            tools_invoked: result.tools_invoked.clone(),
                        });
                    }
                    VerificationResult::EscalateHuman { reason } => {
                        // 需要人工介入
                        warn!(
                            target: "crabmate",
                            "[HIERARCHICAL] Executor: Goal {} requires human escalation: {}",
                            current_goal.goal_id,
                            reason
                        );
                        return Ok(TaskResult {
                            task_id: current_goal.goal_id.clone(),
                            status: TaskStatus::Skipped {
                                reason: format!("Requires human escalation: {}", reason),
                            },
                            output: result.output,
                            error: Some(format!("Requires human escalation: {}", reason)),
                            artifacts: result.artifacts,
                            duration_ms: result.duration_ms,
                            tools_invoked: result.tools_invoked.clone(),
                        });
                    }
                }
            } else {
                // 没有定义验收条件，直接返回执行结果
                if !matches!(result.status, TaskStatus::Failed { .. }) {
                    return Ok(result);
                }
            }

            // 执行失败，尝试 Manager 决策（原有逻辑）
            // Analyze 类型的子目标：失败后直接跳过，不重试
            if matches!(current_goal.goal_type, GoalType::Analyze) {
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
                    tools_invoked: result.tools_invoked.clone(),
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
                super::super::manager::ManagerDecision::Retry { updated_goal } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to retry (attempt {}/{})" , attempt + 1, max_retries);
                    current_goal = *updated_goal;
                    continue;
                }
                super::super::manager::ManagerDecision::Skip { reason } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to skip: {}", reason);
                    return Ok(TaskResult {
                        task_id: current_goal.goal_id.clone(),
                        status: TaskStatus::Skipped { reason },
                        output: result.output,
                        error: result.error,
                        artifacts: result.artifacts,
                        duration_ms: result.duration_ms,
                        tools_invoked: result.tools_invoked.clone(),
                    });
                }
                super::super::manager::ManagerDecision::Abort { reason } => {
                    info!(target: "crabmate", "[HIERARCHICAL] Executor: Manager decided to abort: {}", reason);
                    return Err(ExecutionError::MaxFailuresReached(reason));
                }
            }
        }

        // 达到最大重试次数，返回最后一次失败结果
        info!(target: "crabmate", "[HIERARCHICAL] Executor: max retries ({}) reached for goal_id={}", max_retries, current_goal.goal_id);
        Ok(TaskResult {
            task_id: current_goal.goal_id.clone(),
            status: TaskStatus::Failed {
                reason: format!("Max retries ({}) reached", max_retries),
            },
            output: None,
            error: Some(format!("Max retries ({}) reached", max_retries)),
            artifacts: Vec::new(),
            duration_ms: 0,
            tools_invoked: Vec::new(),
        })
    }

    /// 执行单个子目标的实际逻辑
    async fn execute_single_impl(
        &self,
        goal: &SubGoal,
        prior_subgoals: &[TaskResult],
        artifact_store: &mut ArtifactStore,
        build_state: &BuildState,
    ) -> Result<TaskResult, ExecutionError> {
        // 获取依赖的 artifacts 并按 I/O 契约与类型**裁剪**（默认排除 buildlog/纯 CommandOutput 等）
        let raw = artifact_store.get_dependencies(&goal.depends_on);
        let deps: Vec<_> =
            super::super::subgoal_context::filter_dependencies_for_injection(goal, &raw);
        if deps.len() < raw.len() {
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] Executor: dependency injection filtered {} -> {} artifacts for goal_id={}",
                raw.len(),
                deps.len(),
                goal.goal_id
            );
        }

        info!(
            target: "crabmate",
            "[HIERARCHICAL] Executor: executing goal_id={} desc={} deps_injected={} tools={:?}",
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
        self.emit_subgoal_started_timeline(&goal.goal_id, &goal.description, &goal.required_tools)
            .await;

        // 构建 Operator 配置
        let mut allowed_tools = goal.required_tools.clone();
        // Manager 给出的 `required_tools` 非空时按子集执行；若漏掉构建/运行类子目标常用工具，会导致「Tool run_command is not allowed」等无意义空转
        supplement_subgoal_required_tools(&goal.description, &mut allowed_tools);
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
            max_iterations: 15,
            allowed_tools: allowed_tools.clone(),
            tools_defs: tools_defs_for_llm.clone(),
            sse_out: self.sse_out.clone(),
            artifact_store: Some(artifact_store.clone()),
            build_state: Some(Arc::new(StdMutex::new(build_state.clone()))),
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            attempted_configs: Vec::new(),
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 40,
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
                let mut tool_executor_ctx = super::super::tool_executor::ToolExecutorContext::new(
                    Arc::new(cfg.clone()),
                    work_dir.clone(),
                );
                tool_executor_ctx = tool_executor_ctx.with_probe_cache(self.probe_cache.clone());
                // 如果有审批上下文，启用 Web 审批流程
                if let (Some(out_tx), Some(approval_rx)) = (
                    self.tool_approval_out.clone(),
                    self.tool_approval_rx.clone(),
                ) {
                    tool_executor_ctx =
                        tool_executor_ctx.with_web_approval_arc(out_tx, approval_rx);
                }
                let tool_executor = ToolExecutor::new(tool_executor_ctx);
                let extra = super::super::subgoal_context::build_injected_subgoal_user_extra(
                    goal,
                    &deps,
                    prior_subgoals,
                );
                operator
                    .execute_with_tools(
                        goal,
                        cfg,
                        llm_backend,
                        client,
                        api_key,
                        &tool_executor,
                        extra.as_deref(),
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
                TaskStatus::NeedsDecomposition { .. } => "needs_decomposition",
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
        if matches!(result.status, super::super::task::TaskStatus::Completed) {
            artifact_store.store_result(&result);
        }

        Ok(result)
    }

    /// 询问 Manager 如何处理失败的子目标
    async fn ask_manager_for_decision(
        &self,
        manager: &super::super::manager::ManagerAgent,
        failed_goal: &SubGoal,
        error_message: &str,
        previous_artifacts: &[super::super::task::Artifact],
    ) -> Option<super::super::manager::ManagerDecision> {
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
                ManagerLlmContext {
                    cfg: &cfg,
                    llm_backend,
                    client: &client,
                    api_key: &api_key,
                },
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
                ManagerLlmContext {
                    cfg,
                    llm_backend,
                    client,
                    api_key,
                },
                working_dir,
                &self.tools_defs,
                previous_results,
                &artifacts,
            )
            .await
            .map_err(|e| ExecutionError::MaxFailuresReached(e.to_string()))?;

        Ok(manager_output.sub_goals)
    }

    /// 反思验证失败并尝试重新规划
    ///
    /// 当验证失败时，调用 Manager 进行反思，生成修复策略
    async fn reflect_and_replan(
        &self,
        manager: &super::super::manager::ManagerAgent,
        failed_goal: &SubGoal,
        verification_failure: &str,
        execution_result: &TaskResult,
        artifacts: &[super::super::task::Artifact],
    ) -> Option<SubGoal> {
        info!(
            target: "crabmate",
            "[HIERARCHICAL] Reflecting on verification failure for goal {}: {}",
            failed_goal.goal_id,
            verification_failure
        );

        // 发射 SSE 事件：开始反思
        if let Some(ref sse_out) = self.sse_out {
            let trace =
                events::build_reflection_started_trace(&failed_goal.goal_id, verification_failure);
            let _ = sse::send_string_logged(
                sse_out,
                sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                "hierarchical::reflection_started",
            )
            .await;
        }

        // 提取必要数据
        let cfg = self.cfg.clone()?;
        let llm_backend = self.llm_backend?;
        let client = self.client.as_ref()?.clone();
        let api_key = self.api_key.as_ref()?.clone();
        let working_dir = self.working_dir.as_ref()?.clone();

        // 调用 Manager 进行反思和重新规划
        let reflection_result = manager
            .reflect_and_replan(ReflectAndReplanContext {
                failed_goal,
                verification_failure,
                execution_result,
                cfg: &cfg,
                llm_backend,
                client: &client,
                api_key: &api_key,
                working_dir: &working_dir,
                tools_defs: &self.tools_defs,
                artifacts,
            })
            .await;

        match reflection_result {
            Ok(updated_goal) => {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Reflection successful, updated goal {}",
                    updated_goal.goal_id
                );

                // 发射 SSE 事件：反思完成
                if let Some(ref sse_out) = self.sse_out {
                    let trace = events::build_reflection_finished_trace(&failed_goal.goal_id, true);
                    let _ = sse::send_string_logged(
                        sse_out,
                        sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                        "hierarchical::reflection_finished",
                    )
                    .await;
                }

                Some(updated_goal)
            }
            Err(e) => {
                warn!(
                    target: "crabmate",
                    "[HIERARCHICAL] Reflection failed: {}",
                    e
                );

                // 发射 SSE 事件：反思失败
                if let Some(ref sse_out) = self.sse_out {
                    let trace =
                        events::build_reflection_finished_trace(&failed_goal.goal_id, false);
                    let _ = sse::send_string_logged(
                        sse_out,
                        sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
                        "hierarchical::reflection_failed",
                    )
                    .await;
                }

                None
            }
        }
    }

    /// 从执行结果中更新构建状态
    pub(super) fn update_build_state_from_result(
        &self,
        build_state: &mut BuildState,
        result: &TaskResult,
    ) {
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
