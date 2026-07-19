//! `workflow_execute` 在 `fail_fast` 且首节点失败后须能结束调度（回归 P0 空转）。

use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::config::{AgentConfig, ExposeSecret};
use crabmate::load_config;
use crabmate_workflow::config::WorkflowConfig;

#[tokio::test]
async fn workflow_fail_fast_marks_downstream_skipped_and_returns() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = load_config(None).expect("default config");
    let wf_cfg = workflow_config_from_cfg(&cfg);
    let args = r#"{
        "workflow": {
            "max_parallelism": 1,
            "fail_fast": true,
            "compensate_on_failure": false,
            "nodes": [
                {
                    "id": "fail",
                    "tool_name": "calc",
                    "tool_args": {"expression": "not_a_valid_bc_expr___"},
                    "deps": []
                },
                {
                    "id": "downstream",
                    "tool_name": "get_current_time",
                    "tool_args": {},
                    "deps": ["fail"]
                }
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
    let nodes = v
        .get("nodes")
        .and_then(|x| x.as_array())
        .expect("nodes array");
    let downstream = nodes
        .iter()
        .find(|n| n.get("id").and_then(|x| x.as_str()) == Some("downstream"))
        .expect("downstream node report");
    assert_eq!(
        downstream.get("status").and_then(|x| x.as_str()),
        Some("skipped"),
        "downstream should be skipped under fail_fast: {downstream}"
    );
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
