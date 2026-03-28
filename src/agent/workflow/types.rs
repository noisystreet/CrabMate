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
    /// 最终计入结果的尝试序号（含重试），从 1 起。
    pub(crate) attempt: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct WorkflowTraceEvent {
    /// Unix 毫秒时间戳（调试导出用）。
    pub(crate) timestamp_ms: u64,
    pub(crate) workflow_run_id: u64,
    pub(crate) event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub(crate) workflow_run_id: u64,
    pub(crate) status: String, // passed/failed
    pub(crate) workspace_changed: bool,
    pub(crate) spec: serde_json::Value, // keep flexible: mirror max_parallelism/fail_fast/...
    pub(crate) stats: WorkflowExecutionStats,
    pub(crate) nodes: Vec<WorkflowExecutionNodeReport>,
    pub(crate) first_failure: Option<WorkflowExecutionFirstFailureReport>,
    pub(crate) compensation: WorkflowExecutionCompensationReport,
    /// 按时间顺序的调度/节点事件，便于导出与排障（与日志中 `workflow_run_id` 对齐）。
    pub(crate) trace: Vec<WorkflowTraceEvent>,
    /// 成功节点完成顺序（用于补偿逆序等）；失败运行亦包含已成功完成的 id。
    pub(crate) completion_order: Vec<String>,
    pub(crate) human_summary: String,
}
