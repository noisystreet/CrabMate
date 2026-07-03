//! 子目标顺序/并行执行、单步验证/反思与 `BuildState` 更新（自 `include!` 迁出，便于在仓库内搜索与分层导航）。
//! 由 [`super`]（`execution.rs`）中的 **`execute_with_result`** 调用；本文件仅扩展同一 `HierarchicalExecutor` 的 `impl` 块。

use std::sync::{Arc, Mutex as StdMutex};

use super::super::artifact_resolver::ArtifactResolver;
use super::super::artifact_store::ArtifactStore;
use super::super::build_state::BuildState;
use super::super::events;
use super::super::execution_error::ExecutionError;
use super::super::execution_helpers::truncate_goal_desc;
use super::super::manager::{ManagerLlmContext, ManagerOutput, ReflectAndReplanContext};
use super::super::operator::{OperatorAgent, OperatorConfig};
use super::super::supplement_subgoal_required_tools;
use super::super::task::{SubGoal, TaskResult, TaskStatus};
use super::super::tool_executor::ToolExecutor;
use crate::sse;
use log::{info, warn};

mod execution_parallel;

impl super::HierarchicalExecutor {
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
            if let Some(reason) = super::super::turn_abort::hierarchical_abort_reason(
                self.sse_out.as_ref(),
                self.cancel.as_deref(),
            ) {
                return Err(ExecutionError::TurnAborted(reason));
            }
            let result = self
                .execute_single(goal, &acc, &current_level_ids, artifact_store, build_state)
                .await?;
            acc.push(result.clone());
            results.push(result);
        }

        Ok(results)
    }

    /// 执行单个子目标的实际逻辑
    pub(super) async fn execute_single_impl(
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

        // 根据是否有完整上下文选择执行方法
        let result = if let (
            Some(llm_backend),
            Some(cfg),
            Some(client),
            Some(api_key),
            Some(work_dir),
        ) = (
            self.llm_backend.as_ref(),
            self.cfg.as_ref(),
            self.client.as_ref(),
            self.api_key.as_ref(),
            self.working_dir.as_ref(),
        ) {
            let op_config = OperatorConfig {
                policy: crate::agent::hierarchy::operator::OperatorPolicy {
                    max_iterations: 15,
                    allowed_tools: allowed_tools.clone(),
                    tools_defs: tools_defs_for_llm.clone(),
                    enable_compile_error_recovery: true,
                    compile_error_max_retries: 3,
                    enable_dynamic_decomposition: true,
                    dynamic_decomposition_threshold: 40,
                },
                runtime: crate::agent::hierarchy::operator::OperatorRuntimeHandles {
                    sse_out: self.sse_out.clone(),
                    artifact_store: Some(artifact_store.clone()),
                    build_state: Some(Arc::new(StdMutex::new(build_state.clone()))),
                    cancel: self.cancel.clone(),
                    turn_budget: self.turn_budget.clone(),
                },
            };
            log::info!(
                target: "crabmate",
                "[HIERARCHICAL] execute_single: sse_out is {:?}, tools_defs count={}",
                self.sse_out.is_some(),
                tools_defs_for_llm.len()
            );
            let operator = OperatorAgent::new(op_config);
            // 有完整上下文，使用带工具的执行
            let (hl, sb) = self.require_tool_dispatch_handles()?;
            let mut tool_executor_ctx = super::super::tool_executor::ToolExecutorContext::new(
                Arc::new(cfg.clone()),
                work_dir.clone(),
            )
            .with_dispatch_handles(hl, sb);
            tool_executor_ctx = tool_executor_ctx.with_probe_cache(self.probe_cache.clone());
            // 如果有审批上下文，启用 Web 审批流程
            if let (Some(out_tx), Some(approval_rx)) = (
                self.tool_approval_out.clone(),
                self.tool_approval_rx.clone(),
            ) {
                tool_executor_ctx = tool_executor_ctx.with_web_approval_arc(out_tx, approval_rx);
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
                    llm_backend.as_ref(),
                    client,
                    api_key,
                    &tool_executor,
                    extra.as_deref(),
                )
                .await
                .map_err(ExecutionError::OperatorError)
        } else {
            warn!(
                target: "crabmate",
                "[HIERARCHICAL] execute_single_impl: missing full context for goal_id={}, refusing stub success",
                goal.goal_id
            );
            OperatorAgent::new(OperatorConfig::default())
                .execute(goal)
                .await
                .map_err(ExecutionError::OperatorError)
        };

        let result = result?;

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
    pub(super) async fn ask_manager_for_decision(
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
        let llm_backend = self.llm_backend.as_ref()?.as_ref();
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
                    turn_budget: self.turn_budget.as_ref(),
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

    /// 反思验证失败并尝试重新规划
    ///
    /// 当验证失败时，调用 Manager 进行反思，生成修复策略
    pub(super) async fn reflect_and_replan(
        &self,
        manager: &super::super::manager::ManagerAgent,
        failed_goal: &SubGoal,
        reflect_reason: super::super::reflect_replan_reason::ManagerReflectReplanReason,
        verification_failure: &str,
        execution_result: &TaskResult,
        artifacts: &[super::super::task::Artifact],
    ) -> Option<SubGoal> {
        if !self.try_begin_replan() {
            return None;
        }

        tracing::info!(
            target: "crabmate::hierarchy",
            manager_reflect_replan_reason = reflect_reason.as_str(),
            goal_id = %failed_goal.goal_id,
            verification_failure = %verification_failure,
            "HierarchicalExecutor reflect_and_replan"
        );
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
        let llm_backend = self.llm_backend.as_ref()?.as_ref();
        let client = self.client.as_ref()?.clone();
        let api_key = self.api_key.as_ref()?.clone();
        let working_dir = self.working_dir.as_ref()?.clone();

        // 调用 Manager 进行反思和重新规划
        let reflection_result = manager
            .reflect_and_replan(ReflectAndReplanContext {
                failed_goal,
                verification_failure,
                reflect_reason,
                execution_result,
                cfg: &cfg,
                llm_backend,
                client: &client,
                api_key: &api_key,
                turn_budget: self.turn_budget.as_ref(),
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
    #[allow(clippy::unused_self)] // 保留实例方法形态与既有调用点
    pub(super) fn update_build_state_from_result(
        &self,
        build_state: &mut BuildState,
        result: &TaskResult,
    ) {
        super::execution_build_state_apply::apply_task_result_to_build_state(build_state, result);
    }
}
