//! 将 [`super::types::WorkflowTraceEvent`] 转为 Chrome **Trace Event Format**（JSON 数组），供 `chrome://tracing` 或 [Perfetto](https://ui.perfetto.dev/) 打开。
//!
//! 由环境变量 **`CRABMATE_WORKFLOW_CHROME_TRACE_DIR`**（或 **`AGENT_WORKFLOW_CHROME_TRACE_DIR`**）指定输出目录时，在每次 DAG 执行结束后写入 `workflow-{run_id}-{unix_ms}.json`。
//! **`ts` / `dur` 为微秒**（`displayTimeUnit: "us"`）；时间轴以首条 trace 事件的 `timestamp_ms` 为 0。

use super::types::WorkflowTraceEvent;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;

const ENV_PRIMARY: &str = "CRABMATE_WORKFLOW_CHROME_TRACE_DIR";
const ENV_ALIAS: &str = "AGENT_WORKFLOW_CHROME_TRACE_DIR";

/// 若 `merge_into` 为 `Some`，将工作流事件追加到该缓冲并返回 **`None`**（不写独立 `workflow-*.json`）。
/// 否则若环境变量设置了非空目录，则将 `trace` 写入该目录下的 JSON 文件。
pub(crate) fn maybe_write_workflow_chrome_trace(
    trace: &[WorkflowTraceEvent],
    merge_into: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
) -> Option<String> {
    if let Some(t) = merge_into {
        t.append_workflow_chrome_values(workflow_trace_to_chrome_events_only(trace));
        return None;
    }

    let dir_raw = std::env::var_os(ENV_PRIMARY)
        .or_else(|| std::env::var_os(ENV_ALIAS))
        .and_then(|s| {
            let t = s.to_string_lossy().trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(t))
            }
        })?;

    let first = trace.first()?;
    let workflow_run_id = first.workflow_run_id;
    let t_end_ms = trace
        .last()
        .map(|e| e.timestamp_ms)
        .unwrap_or(first.timestamp_ms);

    let dir = Path::new(&dir_raw);
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!(
            target: "crabmate",
            "workflow chrome trace: create_dir_all failed dir={:?} err={}",
            dir,
            e
        );
        return None;
    }

    let file_name = format!("workflow-{workflow_run_id}-{t_end_ms}.json");
    let path = dir.join(file_name);
    let payload = workflow_trace_to_chrome_json(trace);
    let bytes = match serde_json::to_vec_pretty(&payload) {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "workflow chrome trace: serialize failed workflow_run_id={} err={}",
                workflow_run_id,
                e
            );
            return None;
        }
    };

    match std::fs::File::create(&path).and_then(|mut f| f.write_all(&bytes)) {
        Ok(()) => {
            let path_str = path
                .canonicalize()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            log::info!(
                target: "crabmate",
                "workflow chrome trace written path={} events={}",
                path_str,
                payload.as_array().map(|a| a.len()).unwrap_or(0)
            );
            Some(path_str)
        }
        Err(e) => {
            log::warn!(
                target: "crabmate",
                "workflow chrome trace: write failed path={} err={}",
                path.display(),
                e
            );
            None
        }
    }
}

fn trace_tid(workflow_run_id: u64, node_id: Option<&str>) -> u32 {
    const GLOBAL_TID: u32 = 1;
    let Some(nid) = node_id else {
        return GLOBAL_TID;
    };
    let mut h: u32 = (workflow_run_id as u32) ^ ((workflow_run_id >> 32) as u32);
    for b in nid.bytes() {
        h = h.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    h | 0x10_00
}

fn chrome_cat(ev: &WorkflowTraceEvent) -> &'static str {
    match ev.event.as_str() {
        "dag_start" | "dag_end" => "workflow.meta",
        "compensation_phase_start" | "compensation_phase_end" => "workflow.compensation",
        _ if ev.phase.as_deref() == Some("compensation") => "workflow.compensation",
        "node_run_start" | "node_run_end" => "workflow.node",
        _ => "workflow",
    }
}

fn event_args(ev: &WorkflowTraceEvent) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("workflow_run_id".into(), json!(ev.workflow_run_id));
    if let Some(ref d) = ev.detail {
        m.insert("detail".into(), json!(d));
    }
    if let Some(a) = ev.attempt {
        m.insert("attempt".into(), json!(a));
    }
    if let Some(ref s) = ev.status {
        m.insert("status".into(), json!(s));
    }
    if let Some(ref c) = ev.error_code {
        m.insert("error_code".into(), json!(c));
    }
    if let Some(ref t) = ev.tool_name {
        m.insert("tool".into(), json!(t));
    }
    if let Some(ref p) = ev.phase {
        m.insert("phase".into(), json!(p));
    }
    Value::Object(m)
}

/// 仅工作流事件（无 `process_name` / `trace_config`），供并入整请求 `turn-*.json`。
pub(crate) fn workflow_trace_to_chrome_events_only(trace: &[WorkflowTraceEvent]) -> Vec<Value> {
    let Some(t0_ms) = trace.first().map(|e| e.timestamp_ms) else {
        return vec![];
    };
    let mut out = Vec::with_capacity(trace.len());
    for ev in trace {
        let tid = trace_tid(ev.workflow_run_id, ev.node_id.as_deref());
        let ts_us = ev.timestamp_ms.saturating_sub(t0_ms).saturating_mul(1000);
        let cat = chrome_cat(ev);

        let name = match (&ev.node_id, &ev.tool_name) {
            (Some(nid), Some(tn)) => format!("{} · {} · {}", ev.event, nid, tn),
            (Some(nid), None) => format!("{} · {}", ev.event, nid),
            (None, Some(tn)) => format!("{} · {}", ev.event, tn),
            (None, None) => ev.event.clone(),
        };

        let is_complete = (ev.event == "node_attempt_end" || ev.event == "node_run_end")
            && ev.elapsed_ms.is_some();

        if is_complete {
            let dur_ms = ev.elapsed_ms.unwrap_or(0); // guarded by `is_complete`
            let dur_us = dur_ms.saturating_mul(1000);
            let start_us = ts_us.saturating_sub(dur_us);
            out.push(json!({
                "name": name,
                "cat": cat,
                "ph": "X",
                "ts": start_us,
                "dur": dur_us,
                "pid": 1,
                "tid": tid,
                "args": event_args(ev)
            }));
        } else {
            out.push(json!({
                "name": name,
                "cat": cat,
                "ph": "i",
                "ts": ts_us,
                "pid": 1,
                "tid": tid,
                "s": "t",
                "args": event_args(ev)
            }));
        }
    }
    out
}

/// `ts` / `dur` 使用**微秒**，时间轴以首条事件的 `timestamp_ms` 为 0。
pub(crate) fn workflow_trace_to_chrome_json(trace: &[WorkflowTraceEvent]) -> Value {
    let mut out = Vec::with_capacity(trace.len() + 3);
    out.push(json!({
        "name": "process_name",
        "ph": "M",
        "pid": 1,
        "args": { "name": "CrabMate workflow" }
    }));
    out.push(json!({
        "name": "trace_config",
        "ph": "M",
        "pid": 1,
        "args": { "displayTimeUnit": "us" }
    }));
    out.extend(workflow_trace_to_chrome_events_only(trace));
    Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_trace() -> Vec<WorkflowTraceEvent> {
        vec![
            WorkflowTraceEvent {
                timestamp_ms: 1_700_000_000_000,
                workflow_run_id: 42,
                event: "dag_start".into(),
                node_id: None,
                detail: Some("nodes_count=1".into()),
                attempt: None,
                status: None,
                elapsed_ms: None,
                error_code: None,
                tool_name: None,
                phase: None,
            },
            WorkflowTraceEvent {
                timestamp_ms: 1_700_000_000_100,
                workflow_run_id: 42,
                event: "node_attempt_start".into(),
                node_id: Some("n1".into()),
                detail: None,
                attempt: Some(1),
                status: None,
                elapsed_ms: None,
                error_code: None,
                tool_name: Some("calc".into()),
                phase: Some("main".into()),
            },
            WorkflowTraceEvent {
                timestamp_ms: 1_700_000_000_250,
                workflow_run_id: 42,
                event: "node_attempt_end".into(),
                node_id: Some("n1".into()),
                detail: None,
                attempt: Some(1),
                status: Some("passed".into()),
                elapsed_ms: Some(150),
                error_code: None,
                tool_name: Some("calc".into()),
                phase: Some("main".into()),
            },
        ]
    }

    #[test]
    fn chrome_json_contains_complete_event_for_attempt_end() {
        let v = workflow_trace_to_chrome_json(&sample_trace());
        let arr = v.as_array().expect("array");
        let complete: Vec<_> = arr
            .iter()
            .filter(|e| e.get("ph").and_then(|x| x.as_str()) == Some("X"))
            .collect();
        assert_eq!(complete.len(), 1);
        let x = complete[0];
        assert_eq!(x.get("dur").and_then(|d| d.as_u64()), Some(150_000));
        assert_eq!(x.get("ts").and_then(|t| t.as_u64()), Some(100_000));
    }

    #[test]
    fn chrome_json_instant_events_for_non_duration() {
        let v = workflow_trace_to_chrome_json(&sample_trace());
        let arr = v.as_array().expect("array");
        let instants = arr
            .iter()
            .filter(|e| e.get("ph").and_then(|x| x.as_str()) == Some("i"))
            .count();
        assert!(instants >= 2);
    }

    #[test]
    fn node_run_end_maps_to_complete_event() {
        let trace = vec![WorkflowTraceEvent {
            timestamp_ms: 1000,
            workflow_run_id: 1,
            event: "node_run_end".into(),
            node_id: Some("a".into()),
            detail: None,
            attempt: Some(1),
            status: Some("passed".into()),
            elapsed_ms: Some(50),
            error_code: None,
            tool_name: Some("get_current_time".into()),
            phase: Some("main".into()),
        }];
        let v = workflow_trace_to_chrome_json(&trace);
        let x = v
            .as_array()
            .expect("array")
            .iter()
            .find(|e| e.get("ph").and_then(|p| p.as_str()) == Some("X"))
            .expect("complete");
        assert_eq!(x.get("dur").and_then(|d| d.as_u64()), Some(50_000));
    }
}
