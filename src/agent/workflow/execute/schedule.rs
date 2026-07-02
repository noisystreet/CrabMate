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
            match dag_empty_inflight_action(
                spec,
                &active_nodes,
                &mut completed,
                &mut started,
                &for_each_pending,
                &first_failure,
                &tool_exec_ctx,
            ) {
                DagInflightEmptyAction::Break => break,
                DagInflightEmptyAction::Continue => continue,
            }
        }

        let Some(res) = inflight.next().await else {
            continue;
        };
        dag_record_node_completion(
            &res,
            &mut completed,
            &mut completion_order,
            &mut first_failure,
        );
    }

    DagExecutionProgress {
        completed,
        started,
        completion_order,
        first_failure,
    }
}

enum DagInflightEmptyAction {
    Break,
    Continue,
}

fn dag_empty_inflight_action(
    spec: &WorkflowSpec,
    active_nodes: &[WorkflowNodeSpec],
    completed: &mut HashMap<String, NodeRunResult>,
    started: &mut HashSet<String>,
    for_each_pending: &[super::super::model::ForEachPendingSpec],
    first_failure: &Option<NodeRunResult>,
    tool_exec_ctx: &WorkflowToolExecCtx,
) -> DagInflightEmptyAction {
    if dag_schedule_finished(active_nodes, completed, for_each_pending) {
        return DagInflightEmptyAction::Break;
    }
    if spec.fail_fast && first_failure.is_some() {
        dag_mark_remaining_nodes_fail_fast_skipped(active_nodes, completed, started, tool_exec_ctx);
        return DagInflightEmptyAction::Break;
    }
    DagInflightEmptyAction::Continue
}

fn dag_record_node_completion(
    res: &NodeRunResult,
    completed: &mut HashMap<String, NodeRunResult>,
    completion_order: &mut Vec<String>,
    first_failure: &mut Option<NodeRunResult>,
) {
    if res.status == NodeRunStatus::Passed {
        completion_order.push(res.id.clone());
        completed.insert(res.id.clone(), res.clone());
        return;
    }
    if res.status == NodeRunStatus::Skipped {
        completed.insert(res.id.clone(), res.clone());
        return;
    }
    if first_failure.is_none() {
        *first_failure = Some(res.clone());
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

fn dag_schedule_finished(
    nodes: &[WorkflowNodeSpec],
    completed: &HashMap<String, NodeRunResult>,
    for_each_pending: &[super::super::model::ForEachPendingSpec],
) -> bool {
    !for_each_pending.is_empty() || nodes.iter().all(|n| completed.contains_key(&n.id))
}

/// `fail_fast` 且已有首失败后：将尚未完成的节点标为跳过，避免调度器在 `inflight` 为空时 tight-loop。
fn dag_mark_remaining_nodes_fail_fast_skipped(
    nodes: &[WorkflowNodeSpec],
    completed: &mut HashMap<String, NodeRunResult>,
    started: &mut HashSet<String>,
    tool_exec_ctx: &WorkflowToolExecCtx,
) {
    for node in nodes {
        if completed.contains_key(&node.id) {
            continue;
        }
        started.insert(node.id.clone());
        completed.insert(
            node.id.clone(),
            NodeRunResult {
                id: node.id.clone(),
                status: NodeRunStatus::Skipped,
                output: "fail_fast: workflow aborted after first failure".into(),
                workspace_changed: false,
                exit_code: None,
                error_code: Some("workflow_fail_fast_aborted".to_string()),
                attempt: 0,
            },
        );
        workflow_trace_push(WorkflowTracePush {
            trace: &tool_exec_ctx.trace_events,
            workflow_run_id: tool_exec_ctx.workflow_run_id,
            event: "node_fail_fast_skipped",
            node_id: Some(node.id.as_str()),
            detail: Some("fail_fast after upstream failure".to_string()),
            attempt: None,
            status: Some("skipped"),
            elapsed_ms: None,
            error_code: Some("workflow_fail_fast_aborted"),
            tool_name: Some(node.tool_name.as_str()),
            phase: Some("main"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::workflow::model::WorkflowNodeSpec;
    use std::collections::HashMap;
    use std::sync::Arc;

    use secrecy::ExposeSecret;

    fn sample_nodes() -> Vec<WorkflowNodeSpec> {
        vec![
            WorkflowNodeSpec {
                id: "a".into(),
                tool_name: "tool_a".into(),
                tool_args: serde_json::json!({}),
                deps: vec![],
                requires_approval: false,
                timeout_secs: None,
                compensate_with: vec![],
                max_retries: 0,
                node_tool_role: None,
                run_if: None,
            },
            WorkflowNodeSpec {
                id: "b".into(),
                tool_name: "tool_b".into(),
                tool_args: serde_json::json!({}),
                deps: vec!["a".into()],
                requires_approval: false,
                timeout_secs: None,
                compensate_with: vec![],
                max_retries: 0,
                node_tool_role: None,
                run_if: None,
            },
        ]
    }

    fn minimal_tool_exec_ctx() -> WorkflowToolExecCtx {
        let cfg = Arc::new(crate::config::load_config(None).expect("config"));
        let c = cfg.as_ref();
        WorkflowToolExecCtx {
            cfg: Arc::clone(&cfg),
            cfg_command_timeout_secs: c.command_exec.command_timeout_secs,
            cfg_weather_timeout_secs: c.weather_tool.weather_timeout_secs,
            cfg_web_search_timeout_secs: c.web_search.web_search_timeout_secs,
            cfg_web_search_provider: c.web_search.web_search_provider,
            cfg_web_search_api_key: c
                .web_search
                .web_search_api_key
                .expose_secret()
                .to_string(),
            cfg_web_search_max_results: c.web_search.web_search_max_results,
            cfg_http_fetch_timeout_secs: c.http_fetch.http_fetch_timeout_secs,
            cfg_http_fetch_max_response_bytes: c.http_fetch.http_fetch_max_response_bytes,
            cfg_http_fetch_allowed_prefixes: c.http_fetch.http_fetch_allowed_prefixes.clone(),
            cfg_allowed_commands: Arc::clone(&c.command_exec.allowed_commands),
            effective_working_dir: std::path::PathBuf::from("."),
            workspace_is_set: true,
            command_max_output_len: c.command_exec.command_max_output_len,
            test_result_cache_enabled: c.chat_queues_cache.test_result_cache_enabled,
            test_result_cache_max_entries: c.chat_queues_cache.test_result_cache_max_entries,
            codebase_semantic:
                crate::memory::codebase_semantic_index::CodebaseSemanticToolParams::from_agent_config(
                    c,
                ),
            workflow_run_id: 1,
            trace_events: None,
            request_chrome_merge: None,
        }
    }

    #[test]
    fn dag_schedule_finished_requires_all_nodes_completed() {
        let nodes = sample_nodes();
        let mut completed = HashMap::new();
        assert!(!dag_schedule_finished(&nodes, &completed, &[]));
        completed.insert(
            "a".into(),
            NodeRunResult {
                id: "a".into(),
                status: NodeRunStatus::Failed,
                output: "fail".into(),
                workspace_changed: false,
                exit_code: None,
                error_code: None,
                attempt: 1,
            },
        );
        assert!(!dag_schedule_finished(&nodes, &completed, &[]));
    }

    #[test]
    fn dag_mark_remaining_nodes_fail_fast_skipped_marks_incomplete() {
        let nodes = sample_nodes();
        let mut completed = HashMap::new();
        let mut started = HashSet::new();
        completed.insert(
            "a".into(),
            NodeRunResult {
                id: "a".into(),
                status: NodeRunStatus::Failed,
                output: "boom".into(),
                workspace_changed: false,
                exit_code: None,
                error_code: Some("tool_error".into()),
                attempt: 1,
            },
        );
        started.insert("a".into());
        let tool_exec_ctx = minimal_tool_exec_ctx();
        dag_mark_remaining_nodes_fail_fast_skipped(
            &nodes,
            &mut completed,
            &mut started,
            &tool_exec_ctx,
        );
        assert!(dag_schedule_finished(&nodes, &completed, &[]));
        let b = completed.get("b").expect("b skipped");
        assert_eq!(b.status, NodeRunStatus::Skipped);
        assert_eq!(b.error_code.as_deref(), Some("workflow_fail_fast_aborted"));
    }
}
