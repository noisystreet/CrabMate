#![cfg(feature = "server")]
//! 加载期工具名校验对**入口**生效：`workflow_execute` 的 `validate_only` 与真执行都必须在
//! 跑任何节点之前拒掉未注册的 `tool_name`（#1202）。
//!
//! 这里钉的是「入口确实调用了校验」，而不是校验函数本身（那是 `cm_workflow` 的单元测试）。
//! 断言报告 `type` 为 `workflow_validate_error` / `workflow_execute_error` 而非
//! `*_result`：一旦调用点被删除，那个拼错的节点会被真的执行，报告类型随之改变。
//!
//! `workflow` 节点是裸字符串，拼错时只有执行到该节点才报 `unknown_tool` —— 而前序节点
//! 可能已经写过盘，所以「在哪一步失败」正是本文件要保住的性质。

use crabmate::agent::workflow::config::WorkflowConfig;
use crabmate::agent::workflow::{WorkflowApprovalMode, run_workflow_execute_tool};
use crabmate::config::{AgentConfig, ExposeSecret};
use crabmate::load_config;

/// 单个未注册工具名的节点（`cargo_tst`，拼错）；`validate_only` 由参数控制。
fn spec_with_tool(tool_name: &str, validate_only: bool) -> String {
    serde_json::json!({
        "workflow": {
            "max_parallelism": 1,
            "fail_fast": true,
            "validate_only": validate_only,
            "nodes": [
                {"id": "typo_node", "tool_name": tool_name, "tool_args": {}, "deps": []}
            ]
        }
    })
    .to_string()
}

async fn run_execute(args: &str, dir: &std::path::Path) -> (serde_json::Value, bool) {
    let cfg = load_config(None).expect("default config");
    let wf_cfg = workflow_config_from_cfg(&cfg);
    let (json, workspace_changed) = run_workflow_execute_tool(
        args,
        &wf_cfg,
        dir,
        true,
        WorkflowApprovalMode::NoApproval,
        8192,
        None,
        None,
    )
    .await;
    let v = serde_json::from_str(&json).expect("report json");
    (v, workspace_changed)
}

#[tokio::test]
async fn validate_only_rejects_unregistered_tool_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (v, workspace_changed) = run_execute(&spec_with_tool("cargo_tst", true), tmp.path()).await;

    assert_eq!(v["type"], "workflow_validate_error", "got {v}");
    assert_eq!(v["status"], "failed", "got {v}");
    assert!(!workspace_changed, "校验失败不应改动工作区");
    let summary = v["human_summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("typo_node"),
        "错误须含节点 id，got {summary}"
    );
    assert!(
        summary.contains("cargo_tst"),
        "错误须含工具名，got {summary}"
    );
    assert!(
        v.get("nodes").is_none(),
        "校验失败不应产出节点规划（否则说明校验发生在规划之后）：{v}"
    );
}

#[tokio::test]
async fn execute_rejects_unregistered_tool_name_before_running_nodes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (v, workspace_changed) = run_execute(&spec_with_tool("cargo_tst", false), tmp.path()).await;

    assert_eq!(v["type"], "workflow_execute_error", "got {v}");
    assert_eq!(v["status"], "failed", "got {v}");
    assert!(!workspace_changed, "加载期拒绝不应改动工作区");
    let summary = v["human_summary"].as_str().unwrap_or_default();
    assert!(
        summary.contains("typo_node"),
        "错误须含节点 id，got {summary}"
    );
    // 走到执行器才会出现的东西：节点级 trace / stats。
    assert!(
        v.get("trace").is_none(),
        "被拒的 spec 不应进入 DAG 执行（无 trace）：{v}"
    );
}

#[tokio::test]
async fn validate_only_allows_mcp_proxy_tool_name() {
    // 放行 = 加载期不拦。`mcp__*` 能否真正执行是另一件事（执行期仍是 `unknown_tool`），
    // 这里只钉住「不要因为它在运行时注册就在加载期误伤」。
    let tmp = tempfile::tempdir().expect("tempdir");
    let (v, _) = run_execute(&spec_with_tool("mcp__fanalyzer__list", true), tmp.path()).await;

    assert_eq!(v["type"], "workflow_validate_result", "got {v}");
}

/// 与 `tests/workflow_fail_fast_dag.rs` 同形：从默认配置投影出 `WorkflowConfig`。
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
        web_search_proxy: cfg.web_search.web_search_proxy.clone(),
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
