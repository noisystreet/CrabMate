//! 工作流执行期节点状态与 JSON 报告结构体。

use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeRunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone)]
pub(crate) struct NodeRunResult {
    pub(crate) id: String,
    pub(crate) status: NodeRunStatus,
    pub(crate) output: Arc<str>,
    pub(crate) workspace_changed: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) error_code: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct WorkflowExecutionStats {
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) skipped: usize,
}

#[derive(Serialize)]
pub(crate) struct WorkflowExecutionNodeReport {
    pub(crate) id: String,
    pub(crate) status: String, // passed/failed/skipped
    pub(crate) tool_name: String,
    pub(crate) deps: Vec<String>,
    pub(crate) requires_approval: bool,
    pub(crate) timeout_secs: Option<u64>,
    pub(crate) compensate_with: Vec<String>,
    pub(crate) output_preview: String,
    pub(crate) workspace_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<String>,
    pub(crate) planned_layer: Option<usize>,
    pub(crate) max_retries: u32,
    pub(crate) attempt: u32,
}
pub(crate) static WORKFLOW_RUN_SEQ: AtomicU64 = AtomicU64::new(1);
#[derive(Serialize)]
pub(crate) struct WorkflowExecutionFirstFailureReport {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) first_line: String,
}

#[derive(Serialize)]
pub(crate) struct WorkflowExecutionCompensationReport {
    pub(crate) executed: bool,
    pub(crate) summary: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct WorkflowExecutionReport {
    #[serde(rename = "type")]
    pub(crate) report_type: String,
    pub(crate) status: String, // passed/failed
    pub(crate) workspace_changed: bool,
    pub(crate) spec: serde_json::Value, // keep flexible: mirror max_parallelism/fail_fast/...
    pub(crate) stats: WorkflowExecutionStats,
    pub(crate) nodes: Vec<WorkflowExecutionNodeReport>,
    pub(crate) first_failure: Option<WorkflowExecutionFirstFailureReport>,
    pub(crate) compensation: WorkflowExecutionCompensationReport,
    pub(crate) human_summary: String,
}
