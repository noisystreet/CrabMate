//! 执行结果汇总：节点报告 JSON、`human_summary` 文本、首败摘要。

use std::collections::{HashMap, HashSet};

use super::super::model::{WorkflowNodeSpec, WorkflowSpec};
use super::super::types::{
    NodeRunResult, NodeRunStatus, WorkflowExecutionFirstFailureReport, WorkflowExecutionNodeReport,
};
use super::schedule::DagExecutionProgress;

pub(crate) struct NodeReportsBundle {
    pub(crate) reports: Vec<WorkflowExecutionNodeReport>,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) skipped: usize,
}

pub(super) fn build_workflow_node_reports(
    spec: &WorkflowSpec,
    progress: &DagExecutionProgress,
) -> NodeReportsBundle {
    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut skipped: usize = 0;
    let mut node_reports: Vec<WorkflowExecutionNodeReport> = Vec::new();

    for n in spec.nodes.iter() {
        if let Some(r) = progress.completed.get(&n.id) {
            let st = match r.status {
                NodeRunStatus::Passed => {
                    passed += 1;
                    "passed"
                }
                NodeRunStatus::Failed => {
                    failed += 1;
                    "failed"
                }
            };
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: st.to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: truncate_for_summary(&r.output, spec.summary_preview_max_chars),
                workspace_changed: r.workspace_changed,
                exit_code: r.exit_code,
                error_code: r.error_code.clone(),
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: r.attempt,
            });
        } else if progress.started.contains(&n.id) {
            failed += 1;
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "failed".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: "".to_string(),
                workspace_changed: false,
                exit_code: None,
                error_code: Some("workflow_node_missing_result".to_string()),
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: 1,
            });
        } else {
            skipped += 1;
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "skipped".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: "".to_string(),
                workspace_changed: false,
                exit_code: None,
                error_code: None,
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: 1,
            });
        }
    }

    NodeReportsBundle {
        reports: node_reports,
        passed,
        failed,
        skipped,
    }
}

pub(super) fn build_first_failure_report(
    nodes: &HashMap<String, WorkflowNodeSpec>,
    first_failure: &NodeRunResult,
) -> WorkflowExecutionFirstFailureReport {
    let tool_name = nodes
        .get(&first_failure.id)
        .map(|n| n.tool_name.clone())
        .unwrap_or_default();
    let first_line = first_failure
        .output
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    WorkflowExecutionFirstFailureReport {
        id: first_failure.id.clone(),
        tool: tool_name,
        first_line,
    }
}

pub(super) fn format_main_summary(
    spec: &WorkflowSpec,
    completed: &HashMap<String, NodeRunResult>,
    started: &HashSet<String>,
    completion_order: &[String],
    first_failure: Option<&NodeRunResult>,
) -> String {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    for node in spec.nodes.iter() {
        if let Some(r) = completed.get(&node.id) {
            match r.status {
                NodeRunStatus::Passed => passed += 1,
                NodeRunStatus::Failed => failed += 1,
            }
        } else if started.contains(&node.id) {
            // started 但未落在 completed 的情况理论上不会发生（我们会等待 inflight 全部完成）
            failed += 1;
        } else {
            skipped += 1;
        }
    }

    let status = if first_failure.is_some() {
        "failed"
    } else {
        "passed"
    };

    let mut out = String::new();
    out.push_str("workflow_execute summary:\n");
    out.push_str(&format!(
        "- status: {}\n- max_parallelism: {}\n- fail_fast: {}\n- compensate_on_failure: {}\n",
        status, spec.max_parallelism, spec.fail_fast, spec.compensate_on_failure
    ));
    out.push_str(&format!(
        "- stats: passed={}, failed={}, skipped={}\n",
        passed, failed, skipped
    ));

    out.push_str("- node results:\n");
    let mut listed: HashSet<String> = HashSet::new();
    for id in completion_order.iter() {
        if !listed.insert(id.clone()) {
            continue;
        }
        if let Some(r) = completed.get(id) {
            out.push_str(&format!(
                "  - {}: {:?}\n",
                r.id,
                match r.status {
                    NodeRunStatus::Passed => "passed",
                    NodeRunStatus::Failed => "failed",
                }
            ));
            out.push_str(&format!(
                "    output: {}\n",
                truncate_for_summary(&r.output, spec.summary_preview_max_chars)
            ));
        }
    }
    for node in spec.nodes.iter() {
        if listed.contains(&node.id) {
            continue;
        }
        if let Some(r) = completed.get(&node.id) {
            out.push_str(&format!(
                "  - {}: {}\n",
                r.id,
                if r.status == NodeRunStatus::Passed {
                    "passed"
                } else {
                    "failed"
                }
            ));
            out.push_str(&format!(
                "    output: {}\n",
                truncate_for_summary(&r.output, spec.summary_preview_max_chars)
            ));
        } else {
            out.push_str(&format!("  - {}: skipped\n", node.id));
        }
    }

    if let Some(f) = first_failure {
        out.push_str(&format!(
            "\n首个失败节点：{}（tool={}）\n",
            f.id,
            f.output.lines().next().unwrap_or("")
        ));
    }
    out
}

pub(crate) fn truncate_for_summary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let truncated = crate::tools::output_util::truncate_to_char_boundary(s, max_bytes);
    format!("{}... (截断)", truncated)
}
