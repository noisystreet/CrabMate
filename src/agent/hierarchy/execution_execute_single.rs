//! [`super::HierarchicalExecutor::execute_single`]：验证/反思/Manager 决策分支拆函数以降低圈复杂度。

use std::collections::HashSet;

use super::super::artifact_store::ArtifactStore;
use super::super::build_state::BuildState;
use super::super::events;
use super::super::execution_error::ExecutionError;
use super::super::goal_verifier::{GoalVerifier, VerificationResult};
use super::super::manager::ManagerDecision;
use super::super::task::{GoalType, SubGoal, TaskResult, TaskStatus};
use crate::sse;
use log::{info, warn};

enum DecompositionStep {
    /// `continue` 外层重试循环（可能已更新 `current_goal`）
    ContinueLoop,
    /// `return Ok(...)`
    Finish(TaskResult),
    /// 进入验证或失败处理
    Proceed,
}

#[allow(clippy::large_enum_variant)] // 控制流枚举；可读性优先于栈上体积
enum VerificationStep {
    Return(TaskResult),
    RetryWithUpdatedGoal(SubGoal),
}

#[allow(clippy::large_enum_variant)]
enum FailureOutcome {
    Return(TaskResult),
    RetryWith(SubGoal),
    Abort(String),
}

fn verification_failed_task_result(goal_id: &str, result: &TaskResult, reason: &str) -> TaskResult {
    TaskResult {
        task_id: goal_id.to_string(),
        status: TaskStatus::Failed {
            reason: format!("Verification failed: {}", reason),
        },
        output: result.output.clone(),
        error: Some(format!("Verification failed: {}", reason)),
        artifacts: result.artifacts.clone(),
        duration_ms: result.duration_ms,
        tools_invoked: result.tools_invoked.clone(),
    }
}

fn escalation_skipped_task_result(goal_id: &str, result: &TaskResult, reason: &str) -> TaskResult {
    TaskResult {
        task_id: goal_id.to_string(),
        status: TaskStatus::Skipped {
            reason: format!("Requires human escalation: {}", reason),
        },
        output: result.output.clone(),
        error: Some(format!("Requires human escalation: {}", reason)),
        artifacts: result.artifacts.clone(),
        duration_ms: result.duration_ms,
        tools_invoked: result.tools_invoked.clone(),
    }
}

fn max_retries_exceeded_task_result(goal_id: &str, max_retries: usize) -> TaskResult {
    TaskResult {
        task_id: goal_id.to_string(),
        status: TaskStatus::Failed {
            reason: format!("Max retries ({}) reached", max_retries),
        },
        output: None,
        error: Some(format!("Max retries ({}) reached", max_retries)),
        artifacts: Vec::new(),
        duration_ms: 0,
        tools_invoked: Vec::new(),
    }
}

impl super::HierarchicalExecutor {
    /// 执行单个子目标（带验证和重试循环）
    ///
    /// 执行流程：执行 → 验证 → （失败时）反思/重试
    pub(super) async fn execute_single(
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

        let allowed_commands = self
            .cfg
            .as_ref()
            .map(|c| std::sync::Arc::clone(&c.command_exec.allowed_commands))
            .unwrap_or_else(|| std::sync::Arc::from([] as [String; 0]));
        let command_max_output_len = self
            .cfg
            .as_ref()
            .map(|c| c.command_exec.command_max_output_len)
            .unwrap_or(64 * 1024);

        let verifier = self.working_dir.as_ref().map(|dir| {
            GoalVerifier::with_allowed_commands(dir.clone(), allowed_commands)
                .with_command_max_output_len(command_max_output_len)
        });

        for attempt in 0..max_retries {
            let result = self
                .execute_single_impl(
                    &current_goal,
                    prior_subgoals_for_context,
                    artifact_store,
                    build_state,
                )
                .await?;

            match self
                .execute_single_decomposition_step(&mut current_goal, &result, artifact_store)
                .await?
            {
                DecompositionStep::ContinueLoop => continue,
                DecompositionStep::Finish(tr) => return Ok(tr),
                DecompositionStep::Proceed => {}
            }

            if let Some(ref v) = verifier {
                match self
                    .execute_single_verification_step(
                        v,
                        &current_goal,
                        &result,
                        attempt,
                        max_retries,
                        artifact_store,
                    )
                    .await?
                {
                    VerificationStep::Return(tr) => return Ok(tr),
                    VerificationStep::RetryWithUpdatedGoal(g) => {
                        current_goal = g;
                        continue;
                    }
                }
            } else if !matches!(result.status, TaskStatus::Failed { .. }) {
                return Ok(result);
            }

            match self
                .execute_single_after_execution_failure(
                    &current_goal,
                    &result,
                    artifact_store,
                    attempt,
                    max_retries,
                )
                .await?
            {
                FailureOutcome::Return(tr) => return Ok(tr),
                FailureOutcome::Abort(reason) => {
                    return Err(ExecutionError::MaxFailuresReached(reason));
                }
                FailureOutcome::RetryWith(goal) => {
                    current_goal = goal;
                    continue;
                }
            }
        }

        info!(
            target: "crabmate",
            "[HIERARCHICAL] Executor: max retries ({}) reached for goal_id={}",
            max_retries,
            current_goal.goal_id
        );
        Ok(max_retries_exceeded_task_result(
            &current_goal.goal_id,
            max_retries,
        ))
    }

    async fn execute_single_decomposition_step(
        &self,
        current_goal: &mut SubGoal,
        result: &TaskResult,
        artifact_store: &mut ArtifactStore,
    ) -> Result<DecompositionStep, ExecutionError> {
        let TaskStatus::NeedsDecomposition {
            reason,
            suggested_subgoals,
        } = &result.status
        else {
            return Ok(DecompositionStep::Proceed);
        };

        info!(
            target: "crabmate",
            "[HIERARCHICAL] Executor: Goal {} needs decomposition (suggested {} subgoals): {}",
            current_goal.goal_id,
            suggested_subgoals,
            reason
        );

        if let Some(ref manager) = self.manager {
            let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
            let reflection_result = self
                .reflect_and_replan(
                    manager,
                    current_goal,
                    super::super::reflect_replan_reason::ManagerReflectReplanReason::NeedsDecomposition,
                    &format!("任务过于复杂，建议分解为 {} 个子目标", suggested_subgoals),
                    result,
                    &artifacts,
                )
                .await;

            return Ok(match reflection_result {
                Some(updated_goal) => {
                    info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Executor: Manager replanned goal {} for decomposition",
                        current_goal.goal_id
                    );
                    *current_goal = updated_goal;
                    DecompositionStep::ContinueLoop
                }
                None => DecompositionStep::Finish(result.clone()),
            });
        }

        Ok(DecompositionStep::Finish(result.clone()))
    }

    async fn emit_verification_sse(&self, goal_id: &str, verify_result: &VerificationResult) {
        let Some(sse_out) = self.sse_out.as_ref() else {
            return;
        };
        let trace = match verify_result {
            VerificationResult::Pass => events::build_verification_passed_trace(goal_id),
            VerificationResult::Fail { reason } => {
                events::build_verification_failed_trace(goal_id, reason)
            }
            VerificationResult::EscalateHuman { reason } => {
                events::build_verification_escalated_trace(goal_id, reason)
            }
        };
        let _ = sse::send_string_logged(
            sse_out,
            sse::encode_message(crate::sse::SsePayload::ThinkingTrace { trace }),
            "hierarchical::verification",
        )
        .await;
    }

    async fn execute_single_verification_step(
        &self,
        verifier: &GoalVerifier,
        current_goal: &SubGoal,
        result: &TaskResult,
        attempt: usize,
        max_retries: usize,
        artifact_store: &mut ArtifactStore,
    ) -> Result<VerificationStep, ExecutionError> {
        let degraded = self.budget_degradation_active();
        let verify_result = if degraded {
            warn!(
                target: "crabmate",
                "[HIERARCHICAL] Executor: Goal {} using degraded verification (turn budget pressure)",
                current_goal.goal_id
            );
            verifier.verify_degraded(current_goal, result)
        } else {
            verifier.verify(current_goal, result)
        };
        self.emit_verification_sse(&current_goal.goal_id, &verify_result)
            .await;

        match verify_result {
            VerificationResult::Pass => {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Goal {} verification passed{}",
                    current_goal.goal_id,
                    if degraded { " (degraded)" } else { "" }
                );
                Ok(VerificationStep::Return(result.clone()))
            }
            VerificationResult::Fail { reason } => {
                warn!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Goal {} verification failed: {}. Attempt {}/{}",
                    current_goal.goal_id,
                    reason,
                    attempt + 1,
                    max_retries
                );

                if attempt < max_retries - 1
                    && !degraded
                    && let Some(ref manager) = self.manager
                {
                    let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
                    let reflection_result = self
                        .reflect_and_replan(
                            manager,
                            current_goal,
                            super::super::reflect_replan_reason::ManagerReflectReplanReason::GoalVerificationFailed,
                            &reason,
                            result,
                            &artifacts,
                        )
                        .await;

                    return Ok(match reflection_result {
                        Some(updated_goal) => {
                            info!(
                                target: "crabmate",
                                "[HIERARCHICAL] Executor: Replanning goal {}",
                                current_goal.goal_id
                            );
                            VerificationStep::RetryWithUpdatedGoal(updated_goal)
                        }
                        None => VerificationStep::Return(verification_failed_task_result(
                            &current_goal.goal_id,
                            result,
                            &reason,
                        )),
                    });
                }

                Ok(VerificationStep::Return(verification_failed_task_result(
                    &current_goal.goal_id,
                    result,
                    &reason,
                )))
            }
            VerificationResult::EscalateHuman { reason } => {
                warn!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Goal {} requires human escalation: {}",
                    current_goal.goal_id,
                    reason
                );
                Ok(VerificationStep::Return(escalation_skipped_task_result(
                    &current_goal.goal_id,
                    result,
                    &reason,
                )))
            }
        }
    }

    async fn execute_single_after_execution_failure(
        &self,
        current_goal: &SubGoal,
        result: &TaskResult,
        artifact_store: &mut ArtifactStore,
        attempt: usize,
        max_retries: usize,
    ) -> Result<FailureOutcome, ExecutionError> {
        if matches!(current_goal.goal_type, GoalType::Analyze) {
            info!(
                target: "crabmate",
                "[HIERARCHICAL] Executor: Analyze type goal failed, skipping directly: {}",
                current_goal.goal_id
            );
            return Ok(FailureOutcome::Return(TaskResult {
                task_id: current_goal.goal_id.clone(),
                status: TaskStatus::Skipped {
                    reason: result
                        .error
                        .clone()
                        .unwrap_or_else(|| "Analyze goal failed".to_string()),
                },
                output: result.output.clone(),
                error: result.error.clone(),
                artifacts: result.artifacts.clone(),
                duration_ms: result.duration_ms,
                tools_invoked: result.tools_invoked.clone(),
            }));
        }

        let decision = if let Some(ref manager) = self.manager {
            let error_msg = result.error.as_deref().unwrap_or("Unknown error");
            let artifacts: Vec<_> = artifact_store.all().into_iter().cloned().collect();
            match self
                .ask_manager_for_decision(manager, current_goal, error_msg, &artifacts)
                .await
            {
                Some(d) => d,
                None => return Ok(FailureOutcome::Return(result.clone())),
            }
        } else {
            return Ok(FailureOutcome::Return(result.clone()));
        };

        Ok(match decision {
            ManagerDecision::Retry { updated_goal } => {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Manager decided to retry (attempt {}/{})",
                    attempt + 1,
                    max_retries
                );
                FailureOutcome::RetryWith(*updated_goal)
            }
            ManagerDecision::Skip { reason } => {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Manager decided to skip: {}",
                    reason
                );
                FailureOutcome::Return(TaskResult {
                    task_id: current_goal.goal_id.clone(),
                    status: TaskStatus::Skipped { reason },
                    output: result.output.clone(),
                    error: result.error.clone(),
                    artifacts: result.artifacts.clone(),
                    duration_ms: result.duration_ms,
                    tools_invoked: result.tools_invoked.clone(),
                })
            }
            ManagerDecision::Abort { reason } => {
                info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Executor: Manager decided to abort: {}",
                    reason
                );
                FailureOutcome::Abort(reason)
            }
        })
    }
}
