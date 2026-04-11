//! `workflow_execute` 的调度：**PER 反思**（[`super::per_coord::PerCoordinator`]）与 DAG 执行（[`super::workflow`]）同属 Agent 编排，不进入 [`crate::tool_registry`]，避免 `tool_registry → agent` 依赖。

use std::path::Path;
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::request_chrome_trace::RequestTurnTrace;
use crate::tool_registry::ToolRuntime;

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
    let prep = per_coord.prepare_workflow_execute(args);
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
            let (wf_out, wf_ws_changed) = workflow::run_workflow_execute_tool(
                &prep.patched_args,
                cfg.as_ref(),
                effective_working_dir,
                workspace_is_set,
                approval_mode,
                cfg.command_max_output_len,
                request_chrome_merge,
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
