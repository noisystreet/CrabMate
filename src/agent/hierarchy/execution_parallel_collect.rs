//! 并行子目标 [`JoinSet`] 结果收集（自 `execution_parallel` 拆出以满足 nloc 上限）。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc::Sender;
use tokio::task::JoinSet;

use crate::agent::hierarchy::artifact_store::ArtifactStore;
use crate::agent::hierarchy::execution_error::ExecutionError;
use crate::agent::hierarchy::operator::OperatorError;
use crate::agent::hierarchy::task::{TaskResult, TaskStatus};
use log::{error, info, warn};

/// 汇总并行任务统计并在全部失败时返回错误。
pub(super) fn finalize_parallel_results(
    results: Vec<TaskResult>,
    failed_count: usize,
    panicked_count: usize,
) -> Result<Vec<TaskResult>, ExecutionError> {
    let completed_count = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::Completed))
        .count();
    let needs_decomp_count = results
        .iter()
        .filter(|r| matches!(r.status, TaskStatus::NeedsDecomposition { .. }))
        .count();
    info!(
        target: "crabmate",
        "[HIERARCHICAL] Parallel execution summary: {} total, {} completed, {} needs_decomposition, {} failed, {} panicked",
        results.len(),
        completed_count,
        needs_decomp_count,
        failed_count,
        panicked_count
    );
    if completed_count == 0
        && needs_decomp_count == 0
        && !results.is_empty()
        && results
            .iter()
            .all(|r| matches!(r.status, TaskStatus::Failed { .. }))
    {
        return Err(ExecutionError::MaxFailuresReached(format!(
            "All {} parallel tasks failed ({} execution errors, {} panics)",
            results.len(),
            failed_count,
            panicked_count
        )));
    }
    Ok(results)
}

fn merge_parallel_ok_result(
    artifact_store: &mut ArtifactStore,
    goal_id: &str,
    result: TaskResult,
) -> TaskResult {
    if matches!(result.status, TaskStatus::Completed) {
        artifact_store.store_result(&result);
        info!(
            target: "crabmate",
            "[HIERARCHICAL] Parallel: Goal {} completed successfully",
            goal_id
        );
    } else if matches!(result.status, TaskStatus::NeedsDecomposition { .. }) {
        info!(
            target: "crabmate",
            "[HIERARCHICAL] Parallel: Goal {} needs decomposition",
            goal_id
        );
    } else {
        warn!(
            target: "crabmate",
            "[HIERARCHICAL] Parallel: Goal {} failed: {:?}",
            goal_id,
            result.status
        );
    }
    result
}

fn parallel_operator_error_result(goal_id: String, e: OperatorError) -> TaskResult {
    error!(
        target: "crabmate",
        "[HIERARCHICAL] Parallel: Goal {} execution error: {}",
        goal_id, e
    );
    TaskResult {
        task_id: goal_id,
        status: TaskStatus::Failed {
            reason: format!("Execution error: {}", e),
        },
        output: None,
        error: Some(format!("Execution error: {}", e)),
        artifacts: Vec::new(),
        duration_ms: 0,
        tools_invoked: Vec::new(),
    }
}

fn parallel_panic_result(panicked_count: usize, e: impl std::fmt::Display) -> TaskResult {
    error!(
        target: "crabmate",
        "[HIERARCHICAL] Parallel: Task panicked: {}",
        e
    );
    TaskResult {
        task_id: format!("unknown_panicked_{panicked_count}"),
        status: TaskStatus::Failed {
            reason: format!("Task panicked: {}", e),
        },
        output: None,
        error: Some(format!("Task panicked: {}", e)),
        artifacts: Vec::new(),
        duration_ms: 0,
        tools_invoked: Vec::new(),
    }
}

/// 消费 `JoinSet` 直至完成；若检测到取消则 `abort_all` 并返回 [`ExecutionError::TurnAborted`]。
pub(super) async fn drain_parallel_join_set(
    join_set: &mut JoinSet<(String, Result<TaskResult, OperatorError>)>,
    artifact_store: &mut ArtifactStore,
    sse_out: Option<&Sender<String>>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(Vec<TaskResult>, usize, usize), ExecutionError> {
    let mut results = Vec::new();
    let mut failed_count = 0usize;
    let mut panicked_count = 0usize;

    while let Some(join_result) = join_set.join_next().await {
        if let Some(reason) = crate::agent::hierarchy::turn_abort::hierarchical_abort_reason(
            sse_out,
            cancel.map(|c| c.as_ref()),
        ) {
            join_set.abort_all();
            return Err(ExecutionError::TurnAborted(reason));
        }
        match join_result {
            Ok((goal_id, Ok(result))) => {
                results.push(merge_parallel_ok_result(artifact_store, &goal_id, result));
            }
            Ok((goal_id, Err(e))) => {
                failed_count += 1;
                results.push(parallel_operator_error_result(goal_id, e));
            }
            Err(e) => {
                panicked_count += 1;
                results.push(parallel_panic_result(panicked_count, e));
            }
        }
    }

    Ok((results, failed_count, panicked_count))
}
