//! DAG 执行引擎：并行调度、单节点工具调用、审批、补偿与摘要。
//!
//! 与 `run.rs`（入口）、`types`（报告结构）、`placeholders`（参数注入）配合。
//!
//! | 子模块 | 阶段 |
//! |--------|------|
//! | [`trace`] | 轨迹事件写入 |
//! | [`retry`] | 可重试错误判定 |
//! | [`node`] | 单节点：占位符、审批、`run_tool`、按工具类型超时、退避重试 |
//! | [`schedule`] | DAG 并行调度主循环 |
//! | [`report`] | 节点报告与 `human_summary` 文本 |
//! | [`compensation`] | 失败补偿阶段 |

mod compensation;
mod node;
mod report;
mod retry;
mod schedule;
mod trace;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use log::info;
use tokio::sync::{Mutex, mpsc};

use crate::types::CommandApprovalDecision;

use super::model::{WorkflowNodeSpec, WorkflowSpec};
use super::types::{
    WorkflowExecutionCompensationReport, WorkflowExecutionReport, WorkflowExecutionStats,
    WorkflowTraceEvent,
};

use compensation::workflow_compensation_and_human_summary;
use report::{
    NodeReportsBundle, build_first_failure_report, build_workflow_node_reports, format_main_summary,
};
use schedule::dag_run_parallel_schedule_loop;
use trace::{WorkflowTracePush, workflow_trace_push};

pub(crate) use report::truncate_for_summary;
#[cfg(test)]
pub(crate) use retry::workflow_node_failure_retryable;
#[cfg(test)]
pub(crate) use schedule::node_ready;

#[derive(Debug, Clone)]
pub enum WorkflowApprovalMode {
    NoApproval,
    /// SSE 审批通道（Web `/chat/stream` 等）；字段与 `tool_registry::WebToolRuntime` 对齐。
    Interactive {
        out_tx: mpsc::Sender<String>,
        approval_rx: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
        approval_request_guard: Arc<Mutex<()>>,
        persistent_allowlist: Arc<Mutex<HashSet<String>>>,
    },
}
#[derive(Debug, Clone)]
pub(crate) struct WorkflowToolExecCtx {
    pub(crate) cfg_command_timeout_secs: u64,
    pub(crate) cfg_weather_timeout_secs: u64,
    pub(crate) cfg_web_search_timeout_secs: u64,
    pub(crate) cfg_web_search_provider: crate::config::WebSearchProvider,
    pub(crate) cfg_web_search_api_key: String,
    pub(crate) cfg_web_search_max_results: u32,
    pub(crate) cfg_http_fetch_timeout_secs: u64,
    pub(crate) cfg_http_fetch_max_response_bytes: usize,
    pub(crate) cfg_http_fetch_allowed_prefixes: Vec<String>,
    pub(crate) cfg_allowed_commands: Arc<[String]>,
    pub(crate) effective_working_dir: PathBuf,
    pub(crate) workspace_is_set: bool,
    pub(crate) command_max_output_len: usize,
    pub(crate) test_result_cache_enabled: bool,
    pub(crate) test_result_cache_max_entries: usize,
    /// 与主 Agent 同源，供 `codebase_semantic_search` 等工具在节点内使用。
    pub(crate) codebase_semantic: crate::codebase_semantic_index::CodebaseSemanticToolParams,
    pub(crate) workflow_run_id: u64,
    /// 与本次 DAG 执行共享的轨迹缓冲（`execute_workflow_dag` 内创建）。
    pub(crate) trace_events: Option<Arc<StdMutex<Vec<WorkflowTraceEvent>>>>,
    /// 与整请求 `turn-*.json` 合并时传入；单独跑 `workflow_execute` 时为 `None`。
    pub(crate) request_chrome_merge: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
}

pub(crate) async fn execute_workflow_dag(
    spec: WorkflowSpec,
    approval_mode: WorkflowApprovalMode,
    mut tool_exec_ctx: WorkflowToolExecCtx,
) -> (String, bool) {
    let workflow_run_id = tool_exec_ctx.workflow_run_id;
    let trace = Arc::new(StdMutex::new(Vec::<WorkflowTraceEvent>::new()));
    tool_exec_ctx.trace_events = Some(trace.clone());
    workflow_trace_push(WorkflowTracePush {
        trace: &Some(trace.clone()),
        workflow_run_id,
        event: "dag_start",
        node_id: None,
        detail: Some(format!(
            "nodes_count={} max_parallelism={} fail_fast={} compensate_on_failure={}",
            spec.nodes.len(),
            spec.max_parallelism,
            spec.fail_fast,
            spec.compensate_on_failure
        )),
        attempt: None,
        status: None,
        elapsed_ms: None,
        error_code: None,
        tool_name: None,
        phase: None,
    });
    info!(
        target: "crabmate",
        "workflow dag execute start workflow_run_id={} nodes_count={} max_parallelism={} fail_fast={} compensate_on_failure={}",
        workflow_run_id,
        spec.nodes.len(),
        spec.max_parallelism,
        spec.fail_fast,
        spec.compensate_on_failure
    );
    let nodes: HashMap<String, WorkflowNodeSpec> = spec
        .nodes
        .iter()
        .cloned()
        .map(|n| (n.id.clone(), n))
        .collect();

    let progress =
        dag_run_parallel_schedule_loop(&spec, approval_mode.clone(), tool_exec_ctx.clone()).await;

    let workspace_changed = progress.completed.values().any(|r| r.workspace_changed);

    let main_summary = format_main_summary(
        &spec,
        &progress.completed,
        &progress.started,
        &progress.completion_order,
        progress.first_failure.as_ref(),
    );

    let status = if progress.first_failure.is_some() {
        "failed".to_string()
    } else {
        "passed".to_string()
    };

    let NodeReportsBundle {
        reports: node_reports,
        passed,
        failed,
        skipped,
    } = build_workflow_node_reports(&spec, &progress);

    let first_failure_report = progress
        .first_failure
        .as_ref()
        .map(|f| build_first_failure_report(&nodes, f));

    let (human_summary, comp_workspace_changed, compensation_summary, compensation_executed) =
        workflow_compensation_and_human_summary(
            &spec,
            &nodes,
            &progress,
            main_summary.as_str(),
            approval_mode,
            &tool_exec_ctx,
            workflow_run_id,
        )
        .await;
    let workspace_changed_final = workspace_changed || comp_workspace_changed;

    let completion_order_out = progress.completion_order.clone();
    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id,
        event: "dag_end",
        node_id: None,
        detail: Some(format!(
            "status={} passed={} failed={} skipped={}",
            status, passed, failed, skipped
        )),
        attempt: None,
        status: Some(status.as_str()),
        elapsed_ms: None,
        error_code: None,
        tool_name: None,
        phase: None,
    });
    let trace_final: Vec<WorkflowTraceEvent> = tool_exec_ctx
        .trace_events
        .as_ref()
        .and_then(|t| t.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();

    let chrome_trace_path = super::chrome_trace::maybe_write_workflow_chrome_trace(
        &trace_final,
        tool_exec_ctx.request_chrome_merge.clone(),
    );

    let report = WorkflowExecutionReport {
        report_type: "workflow_execute_result".to_string(),
        workflow_run_id,
        status,
        workspace_changed: workspace_changed_final,
        spec: serde_json::json!({
            "max_parallelism": spec.max_parallelism,
            "fail_fast": spec.fail_fast,
            "compensate_on_failure": spec.compensate_on_failure,
            "output_inject_max_chars": spec.output_inject_max_chars,
            "nodes_count": spec.nodes.len(),
            "planned_layer_count": spec.cached_layer_count
        }),
        stats: WorkflowExecutionStats {
            passed,
            failed,
            skipped,
        },
        nodes: node_reports,
        first_failure: first_failure_report,
        compensation: WorkflowExecutionCompensationReport {
            executed: compensation_executed,
            summary: compensation_summary,
        },
        trace: trace_final,
        completion_order: completion_order_out,
        human_summary,
        chrome_trace_path,
    };

    let json = serde_json::to_string(&report).unwrap_or_else(|_| report.human_summary.clone());
    info!(
        target: "crabmate",
        "workflow dag execute finished workflow_run_id={} status={} passed={} failed={} skipped={} workspace_changed={}",
        workflow_run_id,
        report.status,
        passed,
        failed,
        skipped,
        workspace_changed_final
    );
    // 与历史行为一致：第二返回值仅反映主 DAG 节点是否改动工作区，不含补偿阶段。
    (json, workspace_changed)
}
