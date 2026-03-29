//! `workflow_execute` 在设置 `CRABMATE_WORKFLOW_CHROME_TRACE_DIR` 时写入 Chrome trace，并在报告 JSON 中带 `chrome_trace_path`。

use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::load_config;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::sync::MutexGuard;

static CHROME_TRACE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct ChromeTraceDirGuard<'a> {
    _lock: MutexGuard<'a, ()>,
    prev: Option<OsString>,
}

impl<'a> ChromeTraceDirGuard<'a> {
    fn new(path: &Path) -> Self {
        let _lock = CHROME_TRACE_ENV_LOCK
            .lock()
            .expect("workflow_chrome_trace tests must run serialized");
        let prev = std::env::var_os("CRABMATE_WORKFLOW_CHROME_TRACE_DIR");
        // SAFETY: 持锁期间其它测试不会并发读写该键。
        unsafe {
            std::env::set_var("CRABMATE_WORKFLOW_CHROME_TRACE_DIR", path.as_os_str());
        }
        Self { _lock, prev }
    }
}

impl Drop for ChromeTraceDirGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: 与 `new` 配对，仍持有 `_lock` 至本 guard 析构。
        unsafe {
            match self.prev.take() {
                Some(v) => std::env::set_var("CRABMATE_WORKFLOW_CHROME_TRACE_DIR", v),
                None => std::env::remove_var("CRABMATE_WORKFLOW_CHROME_TRACE_DIR"),
            }
        }
    }
}

#[tokio::test]
async fn workflow_execute_writes_chrome_trace_and_reports_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let trace_dir = tmp.path().join("traces");
    std::fs::create_dir_all(&trace_dir).expect("mkdir traces");

    let _guard = ChromeTraceDirGuard::new(&trace_dir);

    let cfg = load_config(None).expect("default config");
    let args = r#"{
        "workflow": {
            "max_parallelism": 1,
            "fail_fast": true,
            "compensate_on_failure": false,
            "nodes": [
                {"id": "t", "tool_name": "get_current_time", "tool_args": {}, "deps": [], "compensate_with": []}
            ]
        }
    }"#;

    let (json, _ws) = run_workflow_execute_tool(
        args,
        &cfg,
        tmp.path(),
        true,
        WorkflowApprovalMode::NoApproval,
        8192,
    )
    .await;

    let v: serde_json::Value = serde_json::from_str(&json).expect("report json");
    assert_eq!(
        v.get("type").and_then(|x| x.as_str()),
        Some("workflow_execute_result")
    );
    let path_str = v
        .get("chrome_trace_path")
        .and_then(|x| x.as_str())
        .expect("chrome_trace_path");
    assert!(
        std::path::Path::new(path_str).is_file(),
        "trace file missing: {path_str}"
    );

    let trace_json: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(path_str).expect("open trace")).expect("parse");
    let arr = trace_json.as_array().expect("chrome trace array");
    let has_us = arr.iter().any(|e| {
        e.get("name").and_then(|n| n.as_str()) == Some("trace_config")
            && e.get("args")
                .and_then(|a| a.get("displayTimeUnit"))
                .and_then(|u| u.as_str())
                == Some("us")
    });
    assert!(has_us, "expected trace_config displayTimeUnit us");

    let has_node_run = arr.iter().any(|e| {
        e.get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| n.contains("node_run_end"))
    });
    assert!(has_node_run, "expected node_run_end in chrome trace");
}
