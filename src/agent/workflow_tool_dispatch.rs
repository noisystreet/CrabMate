//! `workflow_execute` 的调度：**PER 反思**（[`super::per_coord::PerCoordinator`]）与 DAG 执行（[`super::workflow`]）同属 Agent 编排，不进入 [`crate::tool_registry`]，避免 `tool_registry → agent` 依赖。

use std::path::Path;
use std::sync::Arc;

use crate::config::{AgentConfig, ExposeSecret};
use crate::request_chrome_trace::RequestTurnTrace;
use crate::tool_registry::ToolRuntime;
use crabmate_workflow::config::WorkflowConfig;

use super::per_coord::PerCoordinator;
use super::workflow;
use super::workflow_reflection_controller;

/// Web / CLI 共用：执行 `workflow_execute`（含 `prepare_workflow_execute` 与可选 DAG 运行），返回工具结果正文与反思注入 JSON。
pub async fn dispatch_workflow_execute_tool(
    runtime: ToolRuntime<'_>,
    per_coord: &mut PerCoordinator,
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    args: &str,
    request_chrome_merge: Option<Arc<RequestTurnTrace>>,
) -> (String, Option<serde_json::Value>) {
    let args = match workflow::resolve_workflow_execute_args(
        args,
        effective_working_dir,
        workspace_is_set,
    ) {
        Ok(resolved) => resolved,
        Err(e) => {
            let report = serde_json::json!({
                "type": "workflow_execute_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": format!("workflow_file 解析失败：{e}")
            });
            return (report.to_string(), None);
        }
    };
    let prep = per_coord.prepare_workflow_execute(&args);
    let reflection_inject = prep.reflection_inject.clone();

    let result = if prep.execute {
        if let Err(contract_err) =
            workflow_reflection_controller::validate_workflow_execute_do_contract(
                &prep.patched_args,
            )
        {
            contract_err.to_string()
        } else {
            let (workspace_changed_ref, approval_mode) = match runtime {
                ToolRuntime::Web {
                    workspace_changed,
                    ctx,
                } => {
                    let mode = if let Some(web_ctx) = ctx {
                        workflow::WorkflowApprovalMode::Interactive {
                            out_tx: web_ctx.out_tx.clone(),
                            approval_rx: web_ctx.approval_rx_shared.clone(),
                            approval_request_guard: web_ctx.approval_request_guard.clone(),
                            persistent_allowlist: web_ctx.persistent_allowlist_shared.clone(),
                        }
                    } else {
                        workflow::WorkflowApprovalMode::NoApproval
                    };
                    (workspace_changed, mode)
                }
                ToolRuntime::Cli {
                    workspace_changed, ..
                } => (
                    workspace_changed,
                    workflow::WorkflowApprovalMode::NoApproval,
                ),
            };
            let wf_cfg = WorkflowConfig {
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
                codebase_semantic_max_file_bytes: cfg
                    .codebase_semantic
                    .codebase_semantic_max_file_bytes,
                codebase_semantic_chunk_max_chars: cfg
                    .codebase_semantic
                    .codebase_semantic_chunk_max_chars,
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
                codebase_semantic_hybrid_alpha: cfg
                    .codebase_semantic
                    .codebase_semantic_hybrid_alpha,
                codebase_semantic_fts_top_n: cfg.codebase_semantic.codebase_semantic_fts_top_n,
                codebase_semantic_hybrid_semantic_pool: cfg
                    .codebase_semantic
                    .codebase_semantic_hybrid_semantic_pool,
            };
            let (wf_out, wf_ws_changed) = workflow::run_workflow_execute_tool(
                &prep.patched_args,
                &wf_cfg,
                effective_working_dir,
                workspace_is_set,
                approval_mode,
                cfg.command_exec.command_max_output_len,
                request_chrome_merge.map(|v| v as Arc<dyn std::any::Any + Send + Sync>),
            )
            .await;
            *workspace_changed_ref |= wf_ws_changed;
            wf_out
        }
    } else {
        prep.skipped_result.clone()
    };

    (result, reflection_inject)
}
