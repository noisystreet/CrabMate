//! [`super::HierarchicalExecutor::execute_with_result`] 的实现：拆出层级循环与 SSE，降低圈复杂度。

use std::collections::HashMap;
use std::time::Instant;

use crate::sse;

use super::super::artifact_store::ArtifactStore;
use super::super::build_state::BuildState;
use super::super::events;
use super::super::execution_helpers::Dag;
use super::super::manager::{ManagerOutput, handle_failure};
use super::super::reflect_replan_reason::ManagerReflectReplanReason;
use super::super::task::{ExecutionStrategy, SubGoal, TaskResult, TaskStatus};
use super::{ExecutionError, HierarchicalExecutionResult};

use log::{error, info, warn};

/// 单层执行所需的可变状态（收敛形参个数，避免 `too_many_arguments`）。
struct HierarchicalLevelScratch<'s> {
    artifact_store: &'s mut ArtifactStore,
    build_state: &'s mut BuildState,
    all_results: &'s mut Vec<TaskResult>,
    answer_phase_emitted: &'s mut bool,
}

fn collect_goal_expected_outputs_map(sub_goals: &[SubGoal]) -> HashMap<String, Vec<String>> {
    sub_goals
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
        .collect()
}

impl super::HierarchicalExecutor {
    /// 执行并返回详细结果
    pub async fn execute_with_result(
        &self,
        manager_output: ManagerOutput,
    ) -> Result<HierarchicalExecutionResult, ExecutionError> {
        let start_time = Instant::now();
        let goal_expected_outputs = collect_goal_expected_outputs_map(&manager_output.sub_goals);
        let sub_goals = manager_output.sub_goals;
        let strategy = manager_output.execution_strategy;

        info!(
            target: "crabmate",
            "Hierarchical execution started: {} goals, strategy={:?}",
            sub_goals.len(),
            strategy
        );

        self.emit_trace_hierarchical_started(sub_goals.len(), &format!("{:?}", strategy))
            .await;

        if sub_goals.is_empty() {
            return Ok(empty_hierarchical_result(start_time, goal_expected_outputs));
        }

        let dag = Dag::build(&sub_goals)?;
        let levels = dag.topological_levels()?;

        info!(
            target: "crabmate",
            "Hierarchical execution: {} goals in {} levels",
            sub_goals.len(),
            levels.len()
        );

        let mut artifact_store = ArtifactStore::new();
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

        for (level_idx, level) in levels.iter().enumerate() {
            if let Some(reason) = super::super::turn_abort::hierarchical_abort_reason(
                self.sse_out.as_ref(),
                self.cancel.as_deref(),
            ) {
                return Err(ExecutionError::TurnAborted(reason));
            }
            self.hierarchical_run_one_level(
                level_idx,
                level,
                &sub_goals,
                strategy,
                HierarchicalLevelScratch {
                    artifact_store: &mut artifact_store,
                    build_state: &mut build_state,
                    all_results: &mut all_results,
                    answer_phase_emitted: &mut answer_phase_emitted,
                },
            )
            .await?;

            self.hierarchical_check_failure_abort(level_idx, &artifact_store, &all_results)?;
        }

        Ok(self
            .hierarchical_finalize_success(
                start_time,
                all_results,
                goal_expected_outputs,
                build_state,
            )
            .await)
    }

    async fn emit_trace_hierarchical_started(&self, n_goals: usize, strategy_dbg: &str) {
        let Some(sse_out) = self.sse_out.as_ref() else {
            return;
        };
        let trace = events::build_hierarchical_started_trace(n_goals, strategy_dbg);
        let _ = sse::send_string_logged(
            sse_out,
            sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
            "hierarchical::started",
        )
        .await;
    }

    /// 并行执行后检查是否存在 `NeedsDecomposition` 结果，对受影响的目标执行 replan + 顺序重试。
    async fn handle_parallel_needs_decomposition(
        &self,
        level_results: &mut [TaskResult],
        level_goals: &[&SubGoal],
        all_results: &[TaskResult],
        artifact_store: &mut ArtifactStore,
        build_state: &mut BuildState,
        answer_phase_emitted: &mut bool,
    ) -> Result<(), ExecutionError> {
        let current_level_ids: std::collections::HashSet<String> =
            level_goals.iter().map(|g| g.goal_id.clone()).collect();

        for result in level_results.iter_mut() {
            let TaskStatus::NeedsDecomposition { reason, .. } = &result.status else {
                continue;
            };
            let Some(goal) = level_goals.iter().find(|g| g.goal_id == result.task_id) else {
                continue;
            };

            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Parallel goal {} needs decomposition, performing sequential replan: {}",
                goal.goal_id,
                reason,
            );

            if let Some(ref manager) = self.manager {
                let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
                let updated_goal = self
                    .reflect_and_replan(
                        manager,
                        goal,
                        ManagerReflectReplanReason::NeedsDecomposition,
                        reason,
                        result,
                        &artifacts,
                    )
                    .await;

                if let Some(updated_goal) = updated_goal {
                    log::info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Replanned goal {} for decomposition, re-executing sequentially",
                        goal.goal_id,
                    );
                    let new_result = self
                        .execute_single(
                            &updated_goal,
                            all_results,
                            &current_level_ids,
                            artifact_store,
                            build_state,
                        )
                        .await?;
                    *result = new_result;
                    if matches!(result.status, TaskStatus::Completed) {
                        artifact_store.store_result(result);
                        self.update_build_state_from_result(build_state, result);
                    }
                    if let Some((title, detail)) = Self::progress_line_for_task_result(result) {
                        self.emit_assistant_progress_delta_sse(
                            answer_phase_emitted,
                            title,
                            Some(detail),
                        )
                        .await;
                    }
                    continue;
                }
            }

            // replan 失败或没有 manager：标记为 Failed
            log::warn!(
                target: "crabmate",
                "[HIERARCHICAL] Cannot replan goal {} (no manager or replan exhausted), marking as Failed",
                goal.goal_id,
            );
            *result = TaskResult {
                task_id: goal.goal_id.clone(),
                status: TaskStatus::Failed {
                    reason: format!("子目标过于复杂，无法重新规划：{}", reason),
                },
                output: None,
                error: Some(format!(
                    "NeedsDecomposition could not be resolved in parallel mode: {}",
                    reason,
                )),
                artifacts: Vec::new(),
                duration_ms: 0,
                tools_invoked: Vec::new(),
            };
        }
        Ok(())
    }

    async fn hierarchical_run_one_level(
        &self,
        level_idx: usize,
        level: &[String],
        sub_goals: &[SubGoal],
        strategy: ExecutionStrategy,
        scratch: HierarchicalLevelScratch<'_>,
    ) -> Result<(), ExecutionError> {
        let HierarchicalLevelScratch {
            artifact_store,
            build_state,
            all_results,
            answer_phase_emitted,
        } = scratch;
        info!(
            target: "crabmate",
            "Executing level {} with {} goals",
            level_idx,
            level.len()
        );

        self.emit_trace_level_started(level_idx, level).await;

        let level_goals: Vec<_> = level
            .iter()
            .filter_map(|id| sub_goals.iter().find(|g| &g.goal_id == id))
            .collect();

        let mut level_results = match strategy {
            ExecutionStrategy::Sequential => {
                self.execute_sequential(&level_goals, all_results, artifact_store, build_state)
                    .await
            }
            ExecutionStrategy::Parallel | ExecutionStrategy::Hybrid => {
                self.execute_parallel(&level_goals, all_results, artifact_store, build_state)
                    .await
            }
        }?;

        // 并行模式下处理 NeedsDecomposition：触发 replan 并顺序重试
        if matches!(
            strategy,
            ExecutionStrategy::Parallel | ExecutionStrategy::Hybrid
        ) {
            self.handle_parallel_needs_decomposition(
                &mut level_results,
                &level_goals,
                all_results,
                artifact_store,
                build_state,
                answer_phase_emitted,
            )
            .await?;
        }

        for result in &level_results {
            if matches!(result.status, TaskStatus::Completed) {
                artifact_store.store_result(result);
                self.update_build_state_from_result(build_state, result);
            }
            all_results.push(result.clone());
            if let Some((title, detail)) = Self::progress_line_for_task_result(result) {
                self.emit_assistant_progress_delta_sse(answer_phase_emitted, title, Some(detail))
                    .await;
            }
        }

        self.emit_trace_level_finished(level_idx, &level_results)
            .await;

        Ok(())
    }

    fn hierarchical_check_failure_abort(
        &self,
        level_idx: usize,
        artifact_store: &ArtifactStore,
        all_results: &[TaskResult],
    ) -> Result<(), ExecutionError> {
        let (_, failed, decision) = handle_failure(all_results, self.max_failures);

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
        }

        if !failed.is_empty()
            && let super::super::manager::FailureDecision::Abort { .. } = decision
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

        Ok(())
    }

    async fn emit_trace_level_started(&self, level_idx: usize, level: &[String]) {
        let Some(sse_out) = self.sse_out.as_ref() else {
            return;
        };
        let trace = events::build_level_started_trace(level_idx, level);
        let _ = sse::send_string_logged(
            sse_out,
            sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
            "hierarchical::level_started",
        )
        .await;
    }

    async fn emit_trace_level_finished(&self, level_idx: usize, level_results: &[TaskResult]) {
        let Some(sse_out) = self.sse_out.as_ref() else {
            return;
        };
        let level_completed = level_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Completed))
            .count();
        let level_failed = level_results
            .iter()
            .filter(|r| matches!(r.status, TaskStatus::Failed { .. }))
            .count();
        let trace = events::build_level_finished_trace(level_idx, level_completed, level_failed);
        let _ = sse::send_string_logged(
            sse_out,
            sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
            "hierarchical::level_finished",
        )
        .await;
    }

    async fn hierarchical_finalize_success(
        &self,
        start_time: Instant,
        all_results: Vec<TaskResult>,
        goal_expected_outputs: HashMap<String, Vec<String>>,
        build_state: BuildState,
    ) -> HierarchicalExecutionResult {
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

        if let Some(ref working_dir) = self.working_dir
            && let Err(e) = build_state.save_to_disk(working_dir)
        {
            warn!(
                target: "crabmate",
                "Failed to save build state: {}",
                e
            );
        }

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

        HierarchicalExecutionResult {
            results: all_results,
            total_duration_ms,
            total_completed,
            total_failed,
            goal_expected_outputs,
        }
    }
}

fn empty_hierarchical_result(
    start_time: Instant,
    goal_expected_outputs: HashMap<String, Vec<String>>,
) -> HierarchicalExecutionResult {
    HierarchicalExecutionResult {
        results: Vec::new(),
        total_duration_ms: start_time.elapsed().as_millis() as u64,
        total_completed: 0,
        total_failed: 0,
        goal_expected_outputs,
    }
}
