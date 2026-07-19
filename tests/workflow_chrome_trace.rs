//! `workflow_execute` 在设置 `CM_WORKFLOW_CHROME_TRACE_DIR` 时写入 Chrome trace，并在报告 JSON 中带 `chrome_trace_path`。

use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::config::{AgentConfig, ExposeSecret};
use crabmate::load_config;
use crabmate_workflow::config::WorkflowConfig;
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
        let prev = std::env::var_os("CM_WORKFLOW_CHROME_TRACE_DIR");
        // SAFETY: 持锁期间其它测试不会并发读写该键。
        unsafe {
            std::env::set_var("CM_WORKFLOW_CHROME_TRACE_DIR", path.as_os_str());
        }
        Self { _lock, prev }
    }
}

impl Drop for ChromeTraceDirGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: 与 `new` 配对，仍持有 `_lock` 至本 guard 析构。
        unsafe {
            match self.prev.take() {
                Some(v) => std::env::set_var("CM_WORKFLOW_CHROME_TRACE_DIR", v),
                None => std::env::remove_var("CM_WORKFLOW_CHROME_TRACE_DIR"),
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
    let wf_cfg = workflow_config_from_cfg(&cfg);
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
        &wf_cfg,
        tmp.path(),
        true,
        WorkflowApprovalMode::NoApproval,
        8192,
        None,
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

fn workflow_config_from_cfg(cfg: &AgentConfig) -> WorkflowConfig {
    WorkflowConfig {
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider.as_str().to_string(),
        web_search_api_key: cfg
            .web_search
            .web_search_api_key
            .expose_secret()
            .to_string(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.clone(),
        allowed_commands: cfg.command_exec.allowed_commands.to_vec(),
        command_max_output_len: cfg.command_exec.command_max_output_len,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        codebase_semantic_enabled: cfg.codebase_semantic.codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: cfg
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change,
        codebase_semantic_index_sqlite_path: cfg
            .codebase_semantic
            .codebase_semantic_index_sqlite_path
            .clone(),
        codebase_semantic_max_file_bytes: cfg.codebase_semantic.codebase_semantic_max_file_bytes,
        codebase_semantic_chunk_max_chars: cfg.codebase_semantic.codebase_semantic_chunk_max_chars,
        codebase_semantic_top_k: cfg.codebase_semantic.codebase_semantic_top_k,
        codebase_semantic_query_max_chunks: cfg
            .codebase_semantic
            .codebase_semantic_query_max_chunks,
        codebase_semantic_rebuild_max_files: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_max_files,
        codebase_semantic_rebuild_incremental: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_incremental,
        codebase_semantic_hybrid_alpha: cfg.codebase_semantic.codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: cfg.codebase_semantic.codebase_semantic_fts_top_n,
        codebase_semantic_hybrid_semantic_pool: cfg
            .codebase_semantic
            .codebase_semantic_hybrid_semantic_pool,
    }
}
