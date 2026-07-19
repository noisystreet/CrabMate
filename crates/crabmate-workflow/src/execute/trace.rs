//! 工作流 Chrome / 回合轨迹事件写入（`WorkflowTraceEvent`）。

use std::sync::{Arc, Mutex as StdMutex};

use super::super::types::WorkflowTraceEvent;

pub(super) struct WorkflowTracePush<'a> {
    pub(super) trace: &'a Option<Arc<StdMutex<Vec<WorkflowTraceEvent>>>>,
    pub(super) workflow_run_id: u64,
    pub(super) event: &'a str,
    pub(super) node_id: Option<&'a str>,
    pub(super) detail: Option<String>,
    pub(super) attempt: Option<u32>,
    pub(super) status: Option<&'a str>,
    pub(super) elapsed_ms: Option<u64>,
    pub(super) error_code: Option<&'a str>,
    pub(super) tool_name: Option<&'a str>,
    pub(super) phase: Option<&'a str>,
}

pub(super) fn workflow_trace_push(p: WorkflowTracePush<'_>) {
    let Some(t) = p.trace else {
        return;
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let ev = WorkflowTraceEvent {
        timestamp_ms: ts,
        workflow_run_id: p.workflow_run_id,
        event: p.event.to_string(),
        node_id: p.node_id.map(|s| s.to_string()),
        detail: p.detail,
        attempt: p.attempt,
        status: p.status.map(|s| s.to_string()),
        elapsed_ms: p.elapsed_ms,
        error_code: p.error_code.map(|s| s.to_string()),
        tool_name: p.tool_name.map(|s| s.to_string()),
        phase: p.phase.map(|s| s.to_string()),
    };
    if let Ok(mut g) = t.lock() {
        g.push(ev);
    }
}
