//! 整请求（[`crate::run_agent_turn`]）级 **Chrome Trace Event Format**（`ph: B` / `ph: E`，`ts` 为微秒，原点在回合起点）。
//!
//! 因 **Tokio 任务与 `spawn_blocking` 可能跨线程**，在 **async 主路径**用显式 **B/E** 区间（不依赖 `tracing-subscriber` 全局 Layer），与工作流 [`crate::agent::workflow::chrome_trace`] 事件**合并**到同一文件。
//!
//! | 环境变量 | 说明 |
//! |----------|------|
//! | **`CRABMATE_REQUEST_CHROME_TRACE_DIR`** | 非空目录时，每轮 `run_agent_turn` 结束写入 **`turn-{unix_ms}.json`**。 |
//!
//! 若同时设置 **`CRABMATE_WORKFLOW_CHROME_TRACE_DIR`**（或 **`AGENT_WORKFLOW_CHROME_TRACE_DIR`**），工作流侧**不再单独写** `workflow-*.json`，事件并入本轮 **`turn-*.json`**（`workflow_execute_result.chrome_trace_path` 为 **null**）。

use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 与工作流 layer 对齐：主请求区间使用 **tid=2**。
const REQUEST_TID: u64 = 2;

#[derive(Debug)]
pub struct RequestTurnTrace {
    anchor: Instant,
    events: Mutex<Vec<Value>>,
}

impl RequestTurnTrace {
    pub fn new() -> Self {
        Self {
            anchor: Instant::now(),
            events: Mutex::new(Vec::new()),
        }
    }

    fn ts_us_since_anchor(&self) -> f64 {
        self.anchor.elapsed().as_nanos() as f64 / 1_000.0
    }

    fn push(&self, v: Value) {
        if let Ok(mut g) = self.events.lock() {
            g.push(v);
        }
    }

    /// 进入区间：`name` 为 Chrome `name` 与 B/E 配对名。
    pub fn enter_section(self: &Arc<Self>, name: &'static str) -> TraceSectionGuard {
        let ts = self.ts_us_since_anchor();
        self.push(json!({
            "ph": "B",
            "pid": 1,
            "tid": REQUEST_TID,
            "ts": ts,
            "name": name,
            "cat": "request",
        }));
        TraceSectionGuard {
            trace: Arc::clone(self),
            name,
        }
    }

    /// 追加工作流 Chrome 数组中的对象（已含 `ts` 等）；整体平移到当前缓冲时间轴末尾之后。
    pub fn append_workflow_chrome_values(&self, mut workflow: Vec<Value>) {
        let Ok(mut g) = self.events.lock() else {
            return;
        };
        let max_ts_us = g
            .iter()
            .filter_map(|e| e.get("ts").and_then(|t| t.as_f64()))
            .fold(0.0_f64, f64::max);
        let base = if max_ts_us > 0.0 {
            max_ts_us + 1_000.0
        } else {
            0.0
        };
        for mut ev in workflow.drain(..) {
            if let Some(ts_val) = ev.get_mut("ts") {
                let cur = ts_val.as_f64().unwrap_or(0.0);
                *ts_val = json!(cur + base);
            }
            g.push(ev);
        }
    }

    fn push_exit(&self, name: &'static str) {
        let ts = self.ts_us_since_anchor();
        self.push(json!({
            "ph": "E",
            "pid": 1,
            "tid": REQUEST_TID,
            "ts": ts,
            "name": name,
            "cat": "request",
        }));
    }

    /// 写出 `turn-{wall_ms}.json`；`wall_ms` 为 Unix 毫秒（文件名用）。
    pub fn finish_to_dir(self: &Arc<Self>, wall_start_ms: u64, dir: &std::path::Path) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!(
                target: "crabmate",
                "request chrome trace: create_dir_all failed dir={:?} err={}",
                dir,
                e
            );
            return;
        }
        let path = dir.join(format!("turn-{wall_start_ms}.json"));
        let rows = match self.events.lock() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        let mut out: Vec<Value> = Vec::with_capacity(rows.len() + 4);
        out.push(json!({
            "name": "process_name",
            "ph": "M",
            "pid": 1,
            "args": { "name": "CrabMate request (run_agent_turn)" }
        }));
        out.push(json!({
            "name": "trace_config",
            "ph": "M",
            "pid": 1,
            "args": { "displayTimeUnit": "us" }
        }));
        out.extend(rows);
        let payload = Value::Array(out);
        let bytes = match serde_json::to_vec_pretty(&payload) {
            Ok(b) => b,
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "request chrome trace: serialize err={}",
                    e
                );
                return;
            }
        };
        match std::fs::File::create(&path).and_then(|mut f| f.write_all(&bytes)) {
            Ok(()) => log::info!(
                target: "crabmate",
                "request chrome trace written path={} events={}",
                path.display(),
                payload.as_array().map(|a| a.len()).unwrap_or(0)
            ),
            Err(e) => log::warn!(
                target: "crabmate",
                "request chrome trace: write failed path={} err={}",
                path.display(),
                e
            ),
        }
    }
}

pub struct TraceSectionGuard {
    trace: Arc<RequestTurnTrace>,
    name: &'static str,
}

impl Drop for TraceSectionGuard {
    fn drop(&mut self) {
        self.trace.push_exit(self.name);
    }
}

pub(crate) fn request_trace_dir_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os("CRABMATE_REQUEST_CHROME_TRACE_DIR").and_then(|s| {
        let t = s.to_string_lossy().trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(t))
        }
    })
}

/// 包裹 `fut`，在存在目录配置时创建 [`RequestTurnTrace`] 并在结束后写文件。
pub(crate) async fn with_turn_trace<Fut, T>(
    trace: Arc<RequestTurnTrace>,
    wall_start_ms: u64,
    fut: Fut,
) -> T
where
    Fut: std::future::Future<Output = T>,
{
    let out = fut.await;
    if let Some(dir) = request_trace_dir_from_env() {
        trace.finish_to_dir(wall_start_ms, &dir);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_workflow_shifts_ts_past_request_events() {
        let t = Arc::new(RequestTurnTrace::new());
        let _g = t.enter_section("llm.chat_completions");
        drop(_g);
        let wf = vec![json!({"ph":"i","ts":0.0,"pid":1,"tid":3})];
        t.append_workflow_chrome_values(wf);
        let rows = t.events.lock().expect("lock");
        let last = rows.last().expect("last");
        assert!(last.get("ts").and_then(|x| x.as_f64()).unwrap_or(0.0) > 1000.0);
    }
}
