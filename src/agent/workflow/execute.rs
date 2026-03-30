//! DAG 执行引擎：并行调度、单节点工具调用、审批、补偿与摘要。
//!
//! 与 `run.rs`（入口）、`types`（报告结构）、`placeholders`（参数注入）配合。

use crate::types::CommandApprovalDecision;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use log::info;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore, mpsc};

use super::model::{WorkflowNodeSpec, WorkflowSpec};
use super::placeholders::inject_placeholders;
use super::types::{
    NodeRunResult, NodeRunStatus, WorkflowExecutionCompensationReport,
    WorkflowExecutionFirstFailureReport, WorkflowExecutionNodeReport, WorkflowExecutionReport,
    WorkflowExecutionStats, WorkflowTraceEvent,
};

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

struct WorkflowTracePush<'a> {
    trace: &'a Option<Arc<StdMutex<Vec<WorkflowTraceEvent>>>>,
    workflow_run_id: u64,
    event: &'a str,
    node_id: Option<&'a str>,
    detail: Option<String>,
    attempt: Option<u32>,
    status: Option<&'a str>,
    elapsed_ms: Option<u64>,
    error_code: Option<&'a str>,
    tool_name: Option<&'a str>,
    phase: Option<&'a str>,
}

fn workflow_trace_push(p: WorkflowTracePush<'_>) {
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

/// 是否对节点失败做**自动重试**（保守：避免对业务失败重复执行有副作用工具）。
pub(crate) fn workflow_node_failure_retryable(error_code: Option<&str>) -> bool {
    matches!(
        error_code,
        Some("timeout") | Some("workflow_tool_join_error") | Some("workflow_semaphore_closed")
    )
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

    let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
    let mut started: HashSet<String> = HashSet::new();
    let mut completion_order: Vec<String> = Vec::new();

    let max_parallelism = spec.max_parallelism.max(1);
    let semaphore = Arc::new(Semaphore::new(max_parallelism));
    let mut inflight: FuturesUnordered<_> = FuturesUnordered::new();

    let mut first_failure: Option<NodeRunResult> = None;

    loop {
        // 若 fail_fast 且已有失败，则不再启动新节点，只继续等待已启动节点结束。
        if !(spec.fail_fast && first_failure.is_some()) {
            for node in spec.nodes.iter() {
                if started.contains(&node.id) || completed.contains_key(&node.id) {
                    continue;
                }
                if node_ready(&node.deps, &completed) {
                    started.insert(node.id.clone());
                    let permit_sem = semaphore.clone();
                    let node_cloned = node.clone();
                    let approval_mode_cloned = approval_mode.clone();
                    let exec_ctx = tool_exec_ctx.clone();
                    let completed_snapshot = completed.clone();
                    let inject_max_chars = spec.output_inject_max_chars;
                    let node_id = node_cloned.id.clone();
                    inflight.push(async move {
                        let _permit = match permit_sem.acquire_owned().await {
                            Ok(p) => p,
                            Err(_) => {
                                return NodeRunResult {
                                    id: node_id,
                                    status: NodeRunStatus::Failed,
                                    output: "workflow 并发控制异常（semaphore closed）".into(),
                                    workspace_changed: false,
                                    exit_code: None,
                                    error_code: Some("workflow_semaphore_closed".to_string()),
                                    attempt: 1,
                                };
                            }
                        };
                        run_node(
                            node_cloned,
                            approval_mode_cloned,
                            exec_ctx,
                            completed_snapshot,
                            inject_max_chars,
                            "main",
                        )
                        .await
                    });
                }
            }
        }

        match inflight.next().await {
            None => break,
            Some(res) => {
                if res.status == NodeRunStatus::Passed {
                    completion_order.push(res.id.clone());
                    completed.insert(res.id.clone(), res);
                } else {
                    if first_failure.is_none() {
                        first_failure = Some(res.clone());
                    }
                    // 失败节点不放入 completed；但也要记录到输出里（后面统一拼装）
                    completed.insert(
                        res.id.clone(),
                        NodeRunResult {
                            id: res.id.clone(),
                            status: NodeRunStatus::Failed,
                            output: res.output.clone(),
                            workspace_changed: res.workspace_changed,
                            exit_code: res.exit_code,
                            error_code: res.error_code.clone(),
                            attempt: res.attempt,
                        },
                    );
                }
            }
        }
    }

    let workspace_changed = completed.values().any(|r| r.workspace_changed);

    // 根据 completed/started 组装主结果
    let main_summary = format_main_summary(
        &spec,
        &completed,
        &started,
        &completion_order,
        first_failure.as_ref(),
    );

    let status = if first_failure.is_some() {
        "failed".to_string()
    } else {
        "passed".to_string()
    };

    // 组装节点级报告（按 spec.nodes 的声明顺序）
    let mut passed: usize = 0;
    let mut failed: usize = 0;
    let mut skipped: usize = 0;
    let mut node_reports: Vec<WorkflowExecutionNodeReport> = Vec::new();

    for n in spec.nodes.iter() {
        if let Some(r) = completed.get(&n.id) {
            let st = match r.status {
                NodeRunStatus::Passed => {
                    passed += 1;
                    "passed"
                }
                NodeRunStatus::Failed => {
                    failed += 1;
                    "failed"
                }
            };
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: st.to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: truncate_for_summary(&r.output, spec.summary_preview_max_chars),
                workspace_changed: r.workspace_changed,
                exit_code: r.exit_code,
                error_code: r.error_code.clone(),
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: r.attempt,
            });
        } else if started.contains(&n.id) {
            failed += 1;
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "failed".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: "".to_string(),
                workspace_changed: false,
                exit_code: None,
                error_code: Some("workflow_node_missing_result".to_string()),
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: 1,
            });
        } else {
            skipped += 1;
            node_reports.push(WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "skipped".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: "".to_string(),
                workspace_changed: false,
                exit_code: None,
                error_code: None,
                planned_layer: None,
                max_retries: n.max_retries,
                attempt: 1,
            });
        }
    }

    let first_failure_report = first_failure.as_ref().map(|f| {
        let tool_name = nodes
            .get(&f.id)
            .map(|n| n.tool_name.clone())
            .unwrap_or_default();
        let first_line = f.output.lines().next().unwrap_or("").trim().to_string();
        WorkflowExecutionFirstFailureReport {
            id: f.id.clone(),
            tool: tool_name,
            first_line,
        }
    });

    // 失败补偿（Saga：按成功完成顺序逆序执行补偿节点）
    let mut compensation_summary: Option<String> = None;
    let mut compensation_executed: bool = false;
    let mut workspace_changed_final = workspace_changed;
    let human_summary = if first_failure.is_some() {
        if spec.compensate_on_failure {
            workflow_trace_push(WorkflowTracePush {
                trace: &tool_exec_ctx.trace_events,
                workflow_run_id,
                event: "compensation_phase_start",
                node_id: None,
                detail: None,
                attempt: None,
                status: None,
                elapsed_ms: None,
                error_code: None,
                tool_name: None,
                phase: Some("compensation"),
            });
            let command_max_output_len = command_max_output_len_from(&tool_exec_ctx);
            let (s, comp_workspace_changed) = execute_compensations(
                &spec,
                &nodes,
                &completion_order,
                &completed,
                approval_mode,
                tool_exec_ctx.clone(),
                command_max_output_len,
            )
            .await;
            workspace_changed_final = workspace_changed_final || comp_workspace_changed;
            compensation_summary = Some(s.clone());
            compensation_executed = true;
            workflow_trace_push(WorkflowTracePush {
                trace: &tool_exec_ctx.trace_events,
                workflow_run_id,
                event: "compensation_phase_end",
                node_id: None,
                detail: None,
                attempt: None,
                status: None,
                elapsed_ms: None,
                error_code: None,
                tool_name: None,
                phase: Some("compensation"),
            });
            format!(
                "{}\n\n====================\n\n补偿执行结果：\n{}",
                main_summary, s
            )
        } else {
            format!(
                "{}\n\n补偿已跳过（compensate_on_failure=false）",
                main_summary
            )
        }
    } else {
        main_summary.clone()
    };

    let completion_order_out = completion_order.clone();
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
    (json, workspace_changed)
}

fn command_max_output_len_from(ctx: &WorkflowToolExecCtx) -> usize {
    ctx.command_max_output_len
}

pub(crate) fn node_ready(deps: &[String], completed: &HashMap<String, NodeRunResult>) -> bool {
    deps.iter().all(|d| completed.contains_key(d))
}

/// 审批完成后执行单次工具调用（含 SLA 超时与 `spawn_blocking`）。
async fn execute_node_tool_phase(
    node_id: &str,
    tool_name: &str,
    tool_args_json_str: &str,
    tool_exec_ctx: &WorkflowToolExecCtx,
    effective_allowed_arc: Arc<[String]>,
    timeout_secs: Option<u64>,
) -> NodeRunResult {
    let tool_name_owned = tool_name.to_string();
    let exec_args = tool_args_json_str.to_string();
    let run_command_working_dir = tool_exec_ctx.effective_working_dir.clone();
    let command_max_output_len = tool_exec_ctx.command_max_output_len;
    let weather_timeout_secs = tool_exec_ctx.cfg_weather_timeout_secs;
    let ws_timeout = tool_exec_ctx.cfg_web_search_timeout_secs;
    let ws_provider = tool_exec_ctx.cfg_web_search_provider;
    let ws_max = tool_exec_ctx.cfg_web_search_max_results;
    let ws_key = tool_exec_ctx.cfg_web_search_api_key.clone();
    let hf_pfx = tool_exec_ctx.cfg_http_fetch_allowed_prefixes.clone();
    let hf_to = tool_exec_ctx.cfg_http_fetch_timeout_secs;
    let hf_mb = tool_exec_ctx.cfg_http_fetch_max_response_bytes;
    let test_result_cache_enabled = tool_exec_ctx.test_result_cache_enabled;
    let test_result_cache_max_entries = tool_exec_ctx.test_result_cache_max_entries;
    let codebase_semantic = tool_exec_ctx.codebase_semantic.clone();

    let output_res = async move {
        let work_dir = run_command_working_dir;
        let allowed = effective_allowed_arc;
        let handle = tokio::task::spawn_blocking(move || {
            let ctx = crate::tools::ToolContext {
                codebase_semantic: Some(codebase_semantic),
                command_max_output_len,
                weather_timeout_secs,
                allowed_commands: allowed.as_ref(),
                working_dir: &work_dir,
                web_search_timeout_secs: ws_timeout,
                web_search_provider: ws_provider,
                web_search_api_key: ws_key.as_str(),
                web_search_max_results: ws_max,
                http_fetch_allowed_prefixes: hf_pfx.as_slice(),
                http_fetch_timeout_secs: hf_to,
                http_fetch_max_response_bytes: hf_mb,
                read_file_turn_cache: None,
                workspace_changelist: None,
                test_result_cache_enabled,
                test_result_cache_max_entries,
            };
            crate::tools::run_tool_result(&tool_name_owned, &exec_args, &ctx)
        });
        handle
            .await
            .unwrap_or_else(|e| crate::tool_result::ToolResult {
                ok: false,
                exit_code: None,
                message: format!("工具执行异常：{:?}", e),
                stdout: String::new(),
                stderr: String::new(),
                error_code: Some("workflow_tool_join_error".to_string()),
            })
    };

    let tool_result = if let Some(ts) = timeout_secs {
        match tokio::time::timeout(std::time::Duration::from_secs(ts), output_res).await {
            Ok(s) => s,
            Err(_) => {
                return NodeRunResult {
                    id: node_id.to_string(),
                    status: NodeRunStatus::Failed,
                    output: format!("workflow 节点超时（{} 秒）：tool={}", ts, tool_name).into(),
                    workspace_changed: false,
                    exit_code: None,
                    error_code: Some("timeout".to_string()),
                    attempt: 0,
                };
            }
        }
    } else {
        output_res.await
    };

    let mut workspace_changed = false;
    if tool_name == "run_command"
        && crate::tools::is_compile_command_success(tool_args_json_str, &tool_result.message)
    {
        workspace_changed = true;
    }

    let status = if tool_result.ok {
        NodeRunStatus::Passed
    } else {
        NodeRunStatus::Failed
    };
    let output: Arc<str> = tool_result.message.clone().into();
    NodeRunResult {
        id: node_id.to_string(),
        status,
        output,
        workspace_changed,
        exit_code: tool_result.exit_code,
        error_code: tool_result.error_code.clone(),
        attempt: 0,
    }
}

async fn run_node(
    node: WorkflowNodeSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    completed_snapshot: HashMap<String, NodeRunResult>,
    inject_max_chars: usize,
    phase: &'static str,
) -> NodeRunResult {
    let tool_name = node.tool_name.clone();
    let node_run_wall_start = Instant::now();
    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id: tool_exec_ctx.workflow_run_id,
        event: "node_run_start",
        node_id: Some(node.id.as_str()),
        detail: Some(format!("tool={tool_name} phase={phase}")),
        attempt: None,
        status: None,
        elapsed_ms: None,
        error_code: None,
        tool_name: Some(tool_name.as_str()),
        phase: Some(phase),
    });
    let res = run_node_inner(
        node,
        approval_mode,
        tool_exec_ctx.clone(),
        completed_snapshot,
        inject_max_chars,
        phase,
    )
    .await;
    let st = match res.status {
        NodeRunStatus::Passed => "passed",
        NodeRunStatus::Failed => "failed",
    };
    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id: tool_exec_ctx.workflow_run_id,
        event: "node_run_end",
        node_id: Some(res.id.as_str()),
        detail: None,
        attempt: Some(res.attempt),
        status: Some(st),
        elapsed_ms: Some(node_run_wall_start.elapsed().as_millis() as u64),
        error_code: res.error_code.as_deref(),
        tool_name: Some(tool_name.as_str()),
        phase: Some(phase),
    });
    res
}

async fn run_node_inner(
    node: WorkflowNodeSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    completed_snapshot: HashMap<String, NodeRunResult>,
    inject_max_chars: usize,
    phase: &'static str,
) -> NodeRunResult {
    let node_start = Instant::now();
    info!(
        target: "crabmate",
        "workflow node start workflow_run_id={} node_id={} tool_name={}",
        tool_exec_ctx.workflow_run_id,
        node.id,
        node.tool_name
    );
    // 人工审批：仅对“非 run_command 的人工审批节点”提供通用入口；
    // run_command 的审批仍按 cmd allowlist 逻辑处理。
    let injected_tool_args =
        inject_placeholders(&node.tool_args, &completed_snapshot, inject_max_chars);
    let tool_args_json_str = if injected_tool_args.is_null() {
        "{}".to_string()
    } else {
        injected_tool_args.to_string()
    };
    let mut effective_allowed_arc: Arc<[String]> = Arc::clone(&tool_exec_ctx.cfg_allowed_commands);

    // workspace_is_set 校验（主要覆盖 run_command/run_executable）
    if !tool_exec_ctx.workspace_is_set
        && (node.tool_name == "run_command" || node.tool_name == "run_executable")
    {
        return NodeRunResult {
            id: node.id,
            status: NodeRunStatus::Failed,
            output:
                "错误：未设置工作区，禁止在工作流中执行该工具（需要先在 CLI/Web 设置 workspace）。"
                    .into(),
            workspace_changed: false,
            exit_code: None,
            error_code: Some("workspace_not_set".to_string()),
            attempt: 1,
        };
    }

    // run_command 特殊：按 cmd 白名单 + persistent allowlist 审批
    if node.tool_name == "run_command" {
        if let Some(cmd) = node.tool_args.get("command").and_then(|x| x.as_str()) {
            let cmd_lower = cmd.trim().to_lowercase();
            let disallowed = !tool_exec_ctx
                .cfg_allowed_commands
                .as_ref()
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&cmd_lower));

            let already_allowed = match &approval_mode {
                WorkflowApprovalMode::Interactive {
                    persistent_allowlist,
                    ..
                } => {
                    let guard = persistent_allowlist.lock().await;
                    guard.contains(&cmd_lower)
                }
                WorkflowApprovalMode::NoApproval => false,
            };

            if disallowed && already_allowed && !cmd_lower.is_empty() {
                let mut v: Vec<String> =
                    tool_exec_ctx.cfg_allowed_commands.iter().cloned().collect();
                v.push(cmd_lower.clone());
                effective_allowed_arc = v.into();
            }

            if disallowed && !already_allowed && !cmd_lower.is_empty() {
                // 仅在提供审批通道时才能等待用户决策
                let decision = match &approval_mode {
                    WorkflowApprovalMode::Interactive {
                        out_tx,
                        approval_rx,
                        approval_request_guard,
                        ..
                    } => {
                        let args_preview = node
                            .tool_args
                            .get("args")
                            .and_then(|x| x.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|x| x.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default();
                        request_approval(
                            out_tx.clone(),
                            approval_rx.clone(),
                            approval_request_guard.clone(),
                            &cmd_lower,
                            &args_preview,
                        )
                        .await
                    }
                    WorkflowApprovalMode::NoApproval => {
                        return NodeRunResult {
                            id: node.id,
                            status: NodeRunStatus::Failed,
                            output: format!(
                                "workflow 执行失败：run_command 命令不在允许列表且无法人工审批：{}",
                                cmd_lower
                            )
                            .into(),
                            workspace_changed: false,
                            exit_code: None,
                            error_code: Some("command_not_allowed".to_string()),
                            attempt: 1,
                        };
                    }
                };

                match decision {
                    CommandApprovalDecision::Deny => {
                        return NodeRunResult {
                            id: node.id,
                            status: NodeRunStatus::Failed,
                            output: format!(
                                "workflow 执行失败：用户拒绝执行命令（run_command）：{}",
                                cmd_lower
                            )
                            .into(),
                            workspace_changed: false,
                            exit_code: None,
                            error_code: Some("command_denied".to_string()),
                            attempt: 1,
                        };
                    }
                    CommandApprovalDecision::AllowOnce => {
                        let mut v: Vec<String> =
                            tool_exec_ctx.cfg_allowed_commands.iter().cloned().collect();
                        v.push(cmd_lower.clone());
                        effective_allowed_arc = v.into();
                    }
                    CommandApprovalDecision::AllowAlways => {
                        if let WorkflowApprovalMode::Interactive {
                            persistent_allowlist,
                            ..
                        } = &approval_mode
                        {
                            persistent_allowlist.lock().await.insert(cmd_lower.clone());
                        }
                        let mut v: Vec<String> =
                            tool_exec_ctx.cfg_allowed_commands.iter().cloned().collect();
                        v.push(cmd_lower.clone());
                        effective_allowed_arc = v.into();
                    }
                }
            }
        }
    } else if node.requires_approval {
        // 通用人工审批节点：需 SSE 审批会话
        let approval_key = format!("workflow_node:{}", node.id).to_lowercase();

        match approval_mode {
            WorkflowApprovalMode::NoApproval => {
                return NodeRunResult {
                    id: node.id,
                    status: NodeRunStatus::Failed,
                    output: format!(
                        "workflow 执行失败：该节点需要人工审批，但当前未启用审批通道：{}",
                        approval_key
                    )
                    .into(),
                    workspace_changed: false,
                    exit_code: None,
                    error_code: Some("approval_required".to_string()),
                    attempt: 1,
                };
            }
            WorkflowApprovalMode::Interactive {
                out_tx,
                approval_rx,
                approval_request_guard,
                ref persistent_allowlist,
            } => {
                let already_allowed = persistent_allowlist.lock().await.contains(&approval_key);
                if !already_allowed {
                    let decision = request_approval(
                        out_tx.clone(),
                        approval_rx.clone(),
                        approval_request_guard.clone(),
                        &approval_key,
                        &format!("工具：{}（requires_approval=true）", node.tool_name),
                    )
                    .await;
                    match decision {
                        CommandApprovalDecision::Deny => {
                            return NodeRunResult {
                                id: node.id,
                                status: NodeRunStatus::Failed,
                                output: format!(
                                    "workflow 执行失败：用户拒绝人工审批节点：{}",
                                    approval_key
                                )
                                .into(),
                                workspace_changed: false,
                                exit_code: None,
                                error_code: Some("approval_denied".to_string()),
                                attempt: 1,
                            };
                        }
                        CommandApprovalDecision::AllowOnce => {}
                        CommandApprovalDecision::AllowAlways => {
                            persistent_allowlist.lock().await.insert(approval_key);
                        }
                    }
                }
            }
        }
    }

    // 节点 SLA：timeout_secs 优先；否则按工具类型使用 cfg 默认值（run_command/run_executable 为 command_timeout_secs）
    let timeout_secs = node.timeout_secs.or(match node.tool_name.as_str() {
        "run_command" | "run_executable" => Some(tool_exec_ctx.cfg_command_timeout_secs),
        "maven_compile" | "maven_test" | "gradle_compile" | "gradle_test" | "docker_build"
        | "docker_compose_ps" | "podman_images" => Some(tool_exec_ctx.cfg_command_timeout_secs),
        "get_weather" => Some(tool_exec_ctx.cfg_weather_timeout_secs),
        "web_search" => Some(tool_exec_ctx.cfg_web_search_timeout_secs),
        "http_fetch" | "http_request" => Some(
            tool_exec_ctx
                .cfg_http_fetch_timeout_secs
                .max(tool_exec_ctx.cfg_command_timeout_secs),
        ),
        _ => None,
    });

    workflow_trace_push(WorkflowTracePush {
        trace: &tool_exec_ctx.trace_events,
        workflow_run_id: tool_exec_ctx.workflow_run_id,
        event: "node_ready_execute",
        node_id: Some(node.id.as_str()),
        detail: Some(format!("tool={}", node.tool_name)),
        attempt: None,
        status: None,
        elapsed_ms: None,
        error_code: None,
        tool_name: Some(node.tool_name.as_str()),
        phase: Some(phase),
    });

    let max_attempts = node.max_retries.saturating_add(1).max(1);
    let mut last: Option<NodeRunResult> = None;
    let mut aggregate_workspace_changed = false;
    for attempt in 1..=max_attempts {
        let t0 = Instant::now();
        workflow_trace_push(WorkflowTracePush {
            trace: &tool_exec_ctx.trace_events,
            workflow_run_id: tool_exec_ctx.workflow_run_id,
            event: "node_attempt_start",
            node_id: Some(node.id.as_str()),
            detail: Some(format!("tool={}", node.tool_name)),
            attempt: Some(attempt),
            status: None,
            elapsed_ms: None,
            error_code: None,
            tool_name: Some(node.tool_name.as_str()),
            phase: Some(phase),
        });

        let mut res = execute_node_tool_phase(
            node.id.as_str(),
            node.tool_name.as_str(),
            tool_args_json_str.as_str(),
            &tool_exec_ctx,
            effective_allowed_arc.clone(),
            timeout_secs,
        )
        .await;
        res.attempt = attempt;
        aggregate_workspace_changed |= res.workspace_changed;

        let st = match res.status {
            NodeRunStatus::Passed => "passed",
            NodeRunStatus::Failed => "failed",
        };
        workflow_trace_push(WorkflowTracePush {
            trace: &tool_exec_ctx.trace_events,
            workflow_run_id: tool_exec_ctx.workflow_run_id,
            event: "node_attempt_end",
            node_id: Some(node.id.as_str()),
            detail: None,
            attempt: Some(attempt),
            status: Some(st),
            elapsed_ms: Some(t0.elapsed().as_millis() as u64),
            error_code: res.error_code.as_deref(),
            tool_name: Some(node.tool_name.as_str()),
            phase: Some(phase),
        });

        if res.status == NodeRunStatus::Passed {
            info!(
                target: "crabmate",
                "workflow node finished workflow_run_id={} node_id={} tool_name={} status=Passed attempt={} elapsed_ms={} exit_code={:?}",
                tool_exec_ctx.workflow_run_id,
                res.id,
                node.tool_name,
                attempt,
                node_start.elapsed().as_millis(),
                res.exit_code,
            );
            return res;
        }

        let retryable = workflow_node_failure_retryable(res.error_code.as_deref());
        if attempt < max_attempts && retryable && node.max_retries > 0 {
            let delay = std::cmp::min(2u64.saturating_pow(attempt.saturating_sub(1)), 8);
            workflow_trace_push(WorkflowTracePush {
                trace: &tool_exec_ctx.trace_events,
                workflow_run_id: tool_exec_ctx.workflow_run_id,
                event: "node_retry_backoff",
                node_id: Some(node.id.as_str()),
                detail: Some(format!("sleep_secs={delay} next_attempt={}", attempt + 1)),
                attempt: Some(attempt),
                status: None,
                elapsed_ms: None,
                error_code: res.error_code.as_deref(),
                tool_name: Some(node.tool_name.as_str()),
                phase: Some(phase),
            });
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            last = Some(res);
            continue;
        }
        last = Some(res);
        break;
    }

    let mut result = last.expect("workflow node must produce at least one attempt result");
    result.workspace_changed = aggregate_workspace_changed;
    info!(
        target: "crabmate",
        "workflow node finished workflow_run_id={} node_id={} tool_name={} status={:?} attempts={} elapsed_ms={} exit_code={:?} error_code={:?}",
        tool_exec_ctx.workflow_run_id,
        result.id,
        node.tool_name,
        result.status,
        result.attempt,
        node_start.elapsed().as_millis(),
        result.exit_code,
        result.error_code,
    );
    result
}

async fn request_approval(
    out_tx: mpsc::Sender<String>,
    approval_rx: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    approval_request_guard: Arc<Mutex<()>>,
    command: &str,
    args: &str,
) -> CommandApprovalDecision {
    let spec = crate::tool_approval::ApprovalRequestSpec {
        capability: crate::tool_approval::SensitiveCapability::WorkflowGate,
        sse_command: command.to_string(),
        sse_args: args.to_string(),
        allowlist_key: None,
        cli_title: "工作流审批",
        cli_detail: String::new(),
        web_timeline_prefix_zh: "工作流审批：",
    };
    let sink = crate::tool_approval::WebApprovalSink {
        out_tx: &out_tx,
        approval_rx_shared: &approval_rx,
        approval_request_guard: &approval_request_guard,
    };
    crate::tool_approval::run_web_tool_approval(
        sink,
        &spec,
        "workflow::execute approval request",
        crate::tool_approval::WebApprovalChannelMode::Lenient,
    )
    .await
    .unwrap_or(CommandApprovalDecision::Deny)
}

fn format_main_summary(
    spec: &WorkflowSpec,
    completed: &HashMap<String, NodeRunResult>,
    started: &HashSet<String>,
    completion_order: &[String],
    first_failure: Option<&NodeRunResult>,
) -> String {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    for node in spec.nodes.iter() {
        if let Some(r) = completed.get(&node.id) {
            match r.status {
                NodeRunStatus::Passed => passed += 1,
                NodeRunStatus::Failed => failed += 1,
            }
        } else if started.contains(&node.id) {
            // started 但未落在 completed 的情况理论上不会发生（我们会等待 inflight 全部完成）
            failed += 1;
        } else {
            skipped += 1;
        }
    }

    let status = if first_failure.is_some() {
        "failed"
    } else {
        "passed"
    };

    let mut out = String::new();
    out.push_str("workflow_execute summary:\n");
    out.push_str(&format!(
        "- status: {}\n- max_parallelism: {}\n- fail_fast: {}\n- compensate_on_failure: {}\n",
        status, spec.max_parallelism, spec.fail_fast, spec.compensate_on_failure
    ));
    out.push_str(&format!(
        "- stats: passed={}, failed={}, skipped={}\n",
        passed, failed, skipped
    ));

    out.push_str("- node results:\n");
    let mut listed: HashSet<String> = HashSet::new();
    for id in completion_order.iter() {
        if !listed.insert(id.clone()) {
            continue;
        }
        if let Some(r) = completed.get(id) {
            out.push_str(&format!(
                "  - {}: {:?}\n",
                r.id,
                match r.status {
                    NodeRunStatus::Passed => "passed",
                    NodeRunStatus::Failed => "failed",
                }
            ));
            out.push_str(&format!(
                "    output: {}\n",
                truncate_for_summary(&r.output, spec.summary_preview_max_chars)
            ));
        }
    }
    for node in spec.nodes.iter() {
        if listed.contains(&node.id) {
            continue;
        }
        if let Some(r) = completed.get(&node.id) {
            out.push_str(&format!(
                "  - {}: {}\n",
                r.id,
                if r.status == NodeRunStatus::Passed {
                    "passed"
                } else {
                    "failed"
                }
            ));
            out.push_str(&format!(
                "    output: {}\n",
                truncate_for_summary(&r.output, spec.summary_preview_max_chars)
            ));
        } else {
            out.push_str(&format!("  - {}: skipped\n", node.id));
        }
    }

    if let Some(f) = first_failure {
        out.push_str(&format!(
            "\n首个失败节点：{}（tool={}）\n",
            f.id,
            f.output.lines().next().unwrap_or("")
        ));
    }
    out
}

pub(crate) fn truncate_for_summary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let truncated = crate::tools::output_util::truncate_to_char_boundary(s, max_bytes);
    format!("{}... (截断)", truncated)
}

async fn execute_compensations(
    spec: &WorkflowSpec,
    nodes: &HashMap<String, WorkflowNodeSpec>,
    completion_order: &[String],
    completed: &HashMap<String, NodeRunResult>,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    _command_max_output_len: usize,
) -> (String, bool) {
    let mut compensation_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 按“成功完成节点”的逆序收集 compensate_with
    for id in completion_order.iter().rev() {
        if !completed.contains_key(id) {
            continue;
        }
        if let Some(n) = nodes.get(id) {
            for comp in n.compensate_with.iter() {
                if seen.insert(comp.clone()) {
                    compensation_ids.push(comp.clone());
                }
            }
        }
    }

    if compensation_ids.is_empty() {
        return ("无补偿节点".to_string(), false);
    }

    let mut out = String::new();
    out.push_str(&format!(
        "将执行补偿节点（顺序：逆序收集）：{}\n",
        compensation_ids.join(", ")
    ));

    let mut any_failed = false;
    let mut any_workspace_changed = false;
    for comp_id in compensation_ids {
        let n = match nodes.get(&comp_id) {
            Some(n) => n.clone(),
            None => {
                any_failed = true;
                out.push_str(&format!("- {}: 失败（找不到节点定义）\n", comp_id));
                continue;
            }
        };

        // 补偿节点执行采用串行策略，避免进一步复杂的并发回滚竞态。
        let completed_snapshot = completed.clone();
        let res = run_node(
            n,
            approval_mode.clone(),
            tool_exec_ctx.clone(),
            completed_snapshot,
            spec.output_inject_max_chars,
            "compensation",
        )
        .await;
        if res.status == NodeRunStatus::Passed {
            if res.workspace_changed {
                any_workspace_changed = true;
            }
            out.push_str(&format!("- {}: passed\n", comp_id));
        } else {
            any_failed = true;
            if res.workspace_changed {
                any_workspace_changed = true;
            }
            out.push_str(&format!(
                "- {}: failed\n    output: {}\n",
                comp_id,
                truncate_for_summary(&res.output, spec.compensation_preview_max_chars)
            ));
        }
    }

    if any_failed {
        out.push_str("\n补偿执行存在失败（需要人工介入确认一致性）。");
    }
    (out, any_workspace_changed)
}
