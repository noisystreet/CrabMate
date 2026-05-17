//! DAG 并行调度：就绪检测、choice 剪枝、`for_each` 运行时展开、信号量主循环。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use tokio::sync::Semaphore;

use super::super::for_each_expand::expand_pending_for_each;
use super::super::model::{WorkflowNodeSpec, WorkflowSpec};
use super::super::run_if::{node_deps_resolved, node_run_if_satisfied};
use super::super::types::{NodeRunResult, NodeRunStatus};
use super::node::run_node;
use super::trace::{WorkflowTracePush, workflow_trace_push};
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
    let mut active_nodes = spec.nodes.clone();
    let mut for_each_pending = spec.for_each_pending.clone();
    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    let mut started: HashSet<String> = HashSet::new();
    let mut completion_order: Vec<String> = Vec::new();
    let mut first_failure: Option<NodeRunResult> = None;

    let max_parallelism = spec.max_parallelism.max(1);
    let semaphore = Arc::new(Semaphore::new(max_parallelism));
    let mut inflight: FuturesUnordered<_> = FuturesUnordered::new();

    loop {
        let expanded =
            expand_pending_for_each(&mut for_each_pending, &mut active_nodes, &completed);
        for id in expanded.iter() {
            workflow_trace_push(WorkflowTracePush {
                trace: &tool_exec_ctx.trace_events,
                workflow_run_id: tool_exec_ctx.workflow_run_id,
                event: "for_each_expanded",
                node_id: Some(id.as_str()),
                detail: None,
                attempt: None,
                status: None,
                elapsed_ms: None,
                error_code: None,
                tool_name: None,
                phase: Some("main"),
            });
        }

        if !(spec.fail_fast && first_failure.is_some()) {
            for node in active_nodes.iter() {
                if started.contains(&node.id) || completed.contains_key(&node.id) {
                    continue;
                }
                if !node_deps_resolved(&node.deps, &completed) {
                    continue;
                }
                if !node_run_if_satisfied(node.run_if.as_ref(), &completed) {
                    started.insert(node.id.clone());
                    completed.insert(
                        node.id.clone(),
                        NodeRunResult {
                            id: node.id.clone(),
                            status: NodeRunStatus::Skipped,
                            output: "choice: run_if not satisfied".into(),
                            workspace_changed: false,
                            exit_code: None,
                            error_code: Some("workflow_choice_skipped".to_string()),
                            attempt: 0,
                        },
                    );
                    workflow_trace_push(WorkflowTracePush {
                        trace: &tool_exec_ctx.trace_events,
                        workflow_run_id: tool_exec_ctx.workflow_run_id,
                        event: "node_choice_skipped",
                        node_id: Some(node.id.as_str()),
                        detail: node.run_if.as_ref().map(|_| "run_if=false".to_string()),
                        attempt: None,
                        status: Some("skipped"),
                        elapsed_ms: None,
                        error_code: Some("workflow_choice_skipped"),
                        tool_name: Some(node.tool_name.as_str()),
                        phase: Some("main"),
                    });
                    continue;
                }
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

        if inflight.is_empty() {
            if dag_schedule_finished(&active_nodes, &completed, &for_each_pending) {
                break;
            }
            continue;
        }

        let Some(res) = inflight.next().await else {
            continue;
        };
        {
            if res.status == NodeRunStatus::Passed {
                completion_order.push(res.id.clone());
                completed.insert(res.id.clone(), res);
            } else if res.status == NodeRunStatus::Skipped {
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

    DagExecutionProgress {
        completed,
        started,
        completion_order,
        first_failure,
    }
}

fn dag_schedule_finished(
    nodes: &[WorkflowNodeSpec],
    completed: &HashMap<String, NodeRunResult>,
    for_each_pending: &[super::super::model::ForEachPendingSpec],
) -> bool {
    !for_each_pending.is_empty() || nodes.iter().all(|n| completed.contains_key(&n.id))
}
