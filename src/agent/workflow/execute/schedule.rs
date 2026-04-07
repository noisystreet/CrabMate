//! DAG 并行调度：就绪检测、信号量、`FuturesUnordered` 主循环。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use tokio::sync::Semaphore;

use super::super::model::WorkflowSpec;
use super::super::types::{NodeRunResult, NodeRunStatus};
use super::node::run_node;
use super::{WorkflowApprovalMode, WorkflowToolExecCtx};

/// `execute_workflow_dag` 主调度循环结束后的聚合状态。
pub(crate) struct DagExecutionProgress {
    pub(crate) completed: HashMap<String, NodeRunResult>,
    pub(crate) started: HashSet<String>,
    pub(crate) completion_order: Vec<String>,
    pub(crate) first_failure: Option<NodeRunResult>,
}

/// 并行调度就绪节点并等待全部 inflight 完成。
pub(super) async fn dag_run_parallel_schedule_loop(
    spec: &WorkflowSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
) -> DagExecutionProgress {
    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    let mut started: HashSet<String> = HashSet::new();
    let mut completion_order: Vec<String> = Vec::new();
    let mut first_failure: Option<NodeRunResult> = None;

    let max_parallelism = spec.max_parallelism.max(1);
    let semaphore = Arc::new(Semaphore::new(max_parallelism));
    let mut inflight: FuturesUnordered<_> = FuturesUnordered::new();

    loop {
        if !(spec.fail_fast && first_failure.is_some()) {
            for node in spec.nodes.iter() {
                if started.contains(&node.id) || completed.contains_key(&node.id) {
                    continue;
                }
                if node_ready(&node.deps, &completed) {
                    started.insert(node.id.clone());
                    let permit_sem = semaphore.clone();
                    let node_cloned = node.clone();
                    let approval_mode_cloned = approval_mode.clone();
                    let exec_ctx = tool_exec_ctx.clone();
                    let completed_snapshot = completed.clone();
                    let inject_max_chars = spec.output_inject_max_chars;
                    let node_id = node_cloned.id.clone();
                    inflight.push(async move {
                        let _permit = match permit_sem.acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => {
                                return NodeRunResult {
                                    id: node_id,
                                    status: NodeRunStatus::Failed,
                                    output: "workflow 并发控制异常（semaphore closed）".into(),
                                    workspace_changed: false,
                                    exit_code: None,
                                    error_code: Some("workflow_semaphore_closed".to_string()),
                                    attempt: 1,
                                };
                            }
                        };
                        run_node(
                            node_cloned,
                            approval_mode_cloned,
                            exec_ctx,
                            completed_snapshot,
                            inject_max_chars,
                            "main",
                        )
                        .await
                    });
                }
            }
        }

        match inflight.next().await {
            None => break,
            Some(res) => {
                if res.status == NodeRunStatus::Passed {
                    completion_order.push(res.id.clone());
                    completed.insert(res.id.clone(), res);
                } else {
                    if first_failure.is_none() {
                        first_failure = Some(res.clone());
                    }
                    completed.insert(
                        res.id.clone(),
                        NodeRunResult {
                            id: res.id.clone(),
                            status: NodeRunStatus::Failed,
                            output: res.output.clone(),
                            workspace_changed: res.workspace_changed,
                            exit_code: res.exit_code,
                            error_code: res.error_code.clone(),
                            attempt: res.attempt,
                        },
                    );
                }
            }
        }
    }

    DagExecutionProgress {
        completed,
        started,
        completion_order,
        first_failure,
    }
}

pub(crate) fn node_ready(deps: &[String], completed: &HashMap<String, NodeRunResult>) -> bool {
    deps.iter().all(|d| completed.contains_key(d))
}
