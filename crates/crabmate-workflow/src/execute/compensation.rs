//! 失败后的补偿阶段与 `human_summary` 拼接。

use std::collections::{HashMap, HashSet};

use futures_util::FutureExt;

use super::super::model::{WorkflowNodeSpec, WorkflowSpec};
use super::super::types::{NodeRunResult, NodeRunStatus};
use super::node::{command_max_output_len_from, run_node};
use super::report::truncate_for_summary;
use super::schedule::DagExecutionProgress;
use super::trace::{WorkflowTracePush, workflow_trace_push};
use super::{WorkflowApprovalMode, WorkflowToolExecCtx};

/// 带 catch_unwind 安全包装的补偿节点执行（P0-4），将其从 `execute_compensations` 中提取以降低圈复杂度。
async fn run_compensation_node_safe(
    comp_id: &str,
    node: WorkflowNodeSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    completed_snapshot: HashMap<String, NodeRunResult>,
    inject_max_chars: usize,
) -> NodeRunResult {
    let node_fut = run_node(
        node,
        approval_mode,
        tool_exec_ctx,
        completed_snapshot,
        inject_max_chars,
        "compensation",
    );
    match std::panic::AssertUnwindSafe(node_fut).catch_unwind().await {
        Ok(r) => r,
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                format!("补偿节点 panic：{}", s)
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                format!("补偿节点 panic：{}", s)
            } else {
                "补偿节点 panic（原因未知）".to_string()
            };
            log::error!(
                target: "crabmate",
                "workflow 补偿节点 panic comp_id={} msg={}",
                comp_id,
                msg,
            );
            NodeRunResult {
                id: comp_id.to_string(),
                status: NodeRunStatus::Failed,
                output: msg.into(),
                workspace_changed: false,
                exit_code: None,
                error_code: Some("workflow_node_panic".to_string()),
                attempt: 1,
            }
        }
    }
}

/// 失败时可选补偿阶段，返回 `(human_summary, 补偿是否改动了工作区, compensation_summary, compensation_executed)`。
pub(super) async fn workflow_compensation_and_human_summary(
    spec: &WorkflowSpec,
    nodes: &HashMap<String, WorkflowNodeSpec>,
    progress: &DagExecutionProgress,
    main_summary: &str,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: &WorkflowToolExecCtx,
    workflow_run_id: u64,
) -> (String, bool, Option<String>, bool) {
    if progress.first_failure.is_none() {
        return (main_summary.to_string(), false, None, false);
    }

    if !spec.compensate_on_failure {
        return (
            format!(
                "{}\n\n补偿已跳过（compensate_on_failure=false）",
                main_summary
            ),
            false,
            None,
            false,
        );
    }

    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id,
        event: "compensation_phase_start",
        node_id: None,
        detail: None,
        attempt: None,
        status: None,
        elapsed_ms: None,
        error_code: None,
        tool_name: None,
        phase: Some("compensation"),
    });
    let command_max_output_len = command_max_output_len_from(tool_exec_ctx);
    let (s, comp_workspace_changed) = execute_compensations(
        spec,
        nodes,
        &progress.completion_order,
        &progress.completed,
        approval_mode,
        tool_exec_ctx.clone(),
        command_max_output_len,
    )
    .await;
    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id,
        event: "compensation_phase_end",
        node_id: None,
        detail: None,
        attempt: None,
        status: None,
        elapsed_ms: None,
        error_code: None,
        tool_name: None,
        phase: Some("compensation"),
    });
    let human = format!(
        "{}\n\n====================\n\n补偿执行结果：\n{}",
        main_summary, s
    );
    (human, comp_workspace_changed, Some(s), true)
}

async fn execute_compensations(
    spec: &WorkflowSpec,
    nodes: &HashMap<String, WorkflowNodeSpec>,
    completion_order: &[String],
    completed: &HashMap<String, NodeRunResult>,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    _command_max_output_len: usize,
) -> (String, bool) {
    let mut compensation_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 按"成功完成节点"的逆序收集 compensate_with
    for id in completion_order.iter().rev() {
        if !completed.contains_key(id) {
            continue;
        }
        if let Some(n) = nodes.get(id) {
            for comp in n.compensate_with.iter() {
                if seen.insert(comp.clone()) {
                    compensation_ids.push(comp.clone());
                }
            }
        }
    }

    if compensation_ids.is_empty() {
        return ("无补偿节点".to_string(), false);
    }

    let mut out = String::new();
    out.push_str(&format!(
        "将执行补偿节点（顺序：逆序收集）：{}\n",
        compensation_ids.join(", ")
    ));

    let mut any_failed = false;
    let mut any_workspace_changed = false;
    for comp_id in compensation_ids {
        let n = match nodes.get(&comp_id) {
            Some(n) => n.clone(),
            None => {
                any_failed = true;
                out.push_str(&format!("- {}: 失败（找不到节点定义）\n", comp_id));
                continue;
            }
        };

        // 补偿节点执行采用串行策略，避免进一步复杂的并发回滚竞态。
        let completed_snapshot = completed.clone();
        // P0-4: 使用 catch_unwind 隔离补偿阶段 panic，防止单节点崩溃导致整个补偿阶段丢失
        let res = run_compensation_node_safe(
            &comp_id,
            n,
            approval_mode.clone(),
            tool_exec_ctx.clone(),
            completed_snapshot,
            spec.output_inject_max_chars,
        )
        .await;
        if res.status == NodeRunStatus::Passed {
            if res.workspace_changed {
                any_workspace_changed = true;
            }
            out.push_str(&format!("- {}: passed\n", comp_id));
        } else {
            any_failed = true;
            if res.workspace_changed {
                any_workspace_changed = true;
            }
            out.push_str(&format!(
                "- {}: failed\n    output: {}\n",
                comp_id,
                truncate_for_summary(&res.output, spec.compensation_preview_max_chars)
            ));
        }
    }

    if any_failed {
        out.push_str("\n补偿执行存在失败（需要人工介入确认一致性）。");
    }
    (out, any_workspace_changed)
}
