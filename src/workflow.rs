//! 工作流编排 MVP：DAG 调度 + 并行执行 + 人工审批节点 + 失败补偿 + SLA 超时
//!
//! 当前实现目标是支持模型通过 `workflow_execute` 一次性下发一个 DAG，并由运行时执行引擎在本地完成编排。

use crate::config::AgentConfig;
use crate::types::CommandApprovalDecision;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::{Mutex, Semaphore, mpsc};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct WorkflowSpec {
    pub max_parallelism: usize,
    pub fail_fast: bool,
    pub compensate_on_failure: bool,
    pub output_inject_max_chars: usize,
    pub nodes: Vec<WorkflowNodeSpec>,
}

#[derive(Debug, Clone)]
pub struct WorkflowNodeSpec {
    pub id: String,
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub deps: Vec<String>,
    pub requires_approval: bool,
    pub timeout_secs: Option<u64>,
    pub compensate_with: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeRunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone)]
struct NodeRunResult {
    id: String,
    status: NodeRunStatus,
    output: String,
    workspace_changed: bool,
    exit_code: Option<i32>,
    error_code: Option<String>,
}

#[derive(Serialize)]
struct WorkflowExecutionStats {
    passed: usize,
    failed: usize,
    skipped: usize,
}

#[derive(Serialize)]
struct WorkflowExecutionNodeReport {
    id: String,
    status: String, // passed/failed/skipped
    tool_name: String,
    deps: Vec<String>,
    requires_approval: bool,
    timeout_secs: Option<u64>,
    compensate_with: Vec<String>,
    output_preview: String,
    workspace_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    planned_layer: Option<usize>,
}

static WORKFLOW_RUN_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize)]
struct WorkflowExecutionFirstFailureReport {
    id: String,
    tool: String,
    first_line: String,
}

#[derive(Serialize)]
struct WorkflowExecutionCompensationReport {
    executed: bool,
    summary: Option<String>,
}

#[derive(Serialize)]
struct WorkflowExecutionReport {
    #[serde(rename = "type")]
    report_type: String,
    status: String, // passed/failed
    workspace_changed: bool,
    spec: serde_json::Value, // keep flexible: mirror max_parallelism/fail_fast/...
    stats: WorkflowExecutionStats,
    nodes: Vec<WorkflowExecutionNodeReport>,
    first_failure: Option<WorkflowExecutionFirstFailureReport>,
    compensation: WorkflowExecutionCompensationReport,
    human_summary: String,
}

#[derive(Debug, Clone)]
pub enum WorkflowApprovalMode {
    NoApproval,
    Tui {
        out_tx: mpsc::Sender<String>,
        approval_rx: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
        approval_request_guard: Arc<Mutex<()>>,
        persistent_allowlist: Arc<Mutex<HashSet<String>>>,
    },
}

pub async fn run_workflow_execute_tool(
    args_json: &str,
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    approval_mode: WorkflowApprovalMode,
    command_max_output_len: usize,
) -> (String, bool) {
    let workflow_run_id = WORKFLOW_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    info!(
        workflow_run_id = workflow_run_id,
        workspace_is_set = workspace_is_set,
        "workflow_execute start"
    );
    // 支持反思阶段的“done=true”：运行时应跳过 DAG 执行，
    // 只返回一个明确的结果，避免模型误触发重复执行。
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => {
            warn!(
                workflow_run_id = workflow_run_id,
                "workflow_execute args parse failed"
            );
            let report = serde_json::json!({
                "type": "workflow_execute_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": "workflow_execute 参数解析错误"
            });
            return (report.to_string(), false);
        }
    };
    let workflow_v = v.get("workflow").unwrap_or(&v);

    let done = workflow_v
        .get("done")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if done {
        info!(
            workflow_run_id = workflow_run_id,
            "workflow_execute skip by done=true"
        );
        let report = serde_json::json!({
            "type": "workflow_execute_done_skip",
            "status": "passed",
            "workspace_changed": false,
            "spec": workflow_v.clone(),
            "stats": { "passed": 0, "failed": 0, "skipped": 0 },
            "nodes": [],
            "first_failure": null,
            "compensation": { "executed": false, "summary": null },
            "human_summary": "workflow_execute: reflection done=true，跳过 DAG 执行。"
        });
        return (report.to_string(), false);
    }

    let validate_only = workflow_v
        .get("validate_only")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if validate_only {
        info!(
            workflow_run_id = workflow_run_id,
            "workflow_validate_only start"
        );
        let spec = match parse_workflow_spec(args_json) {
            Ok(s) => s,
            Err(e) => {
                warn!(workflow_run_id = workflow_run_id, error = %e, "workflow_validate_only parse failed");
                let report = serde_json::json!({
                    "type": "workflow_validate_error",
                    "status": "failed",
                    "workspace_changed": false,
                    "human_summary": format!("workflow_validate 参数解析错误：{}", e)
                });
                return (report.to_string(), false);
            }
        };

        if let Err(e) = validate_dag(&spec.nodes) {
            warn!(workflow_run_id = workflow_run_id, error = %e, "workflow_validate_only dag validation failed");
            let report = serde_json::json!({
                "type": "workflow_validate_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": format!("workflow_validate DAG 校验失败：{}", e)
            });
            return (report.to_string(), false);
        }

        // 计算静态执行层（拓扑层级），用于“规划”输出
        let execution_layers = match topo_layers(&spec.nodes) {
            Ok(l) => l,
            Err(e) => {
                warn!(workflow_run_id = workflow_run_id, error = %e, "workflow_validate_only topo layer failed");
                let report = serde_json::json!({
                    "type": "workflow_validate_error",
                    "status": "failed",
                    "workspace_changed": false,
                    "human_summary": format!("workflow_validate 层级计算失败：{}", e)
                });
                return (report.to_string(), false);
            }
        };

        // 反查每个节点所在 layer index
        let mut layer_idx_by_id: HashMap<String, usize> = HashMap::new();
        for (i, layer) in execution_layers.iter().enumerate() {
            for id in layer.iter() {
                layer_idx_by_id.insert(id.clone(), i);
            }
        }

        let node_reports: Vec<WorkflowExecutionNodeReport> = spec
            .nodes
            .iter()
            .map(|n| WorkflowExecutionNodeReport {
                id: n.id.clone(),
                status: "planned".to_string(),
                tool_name: n.tool_name.clone(),
                deps: n.deps.clone(),
                requires_approval: n.requires_approval,
                timeout_secs: n.timeout_secs,
                compensate_with: n.compensate_with.clone(),
                output_preview: truncate_for_summary(&n.tool_args.to_string(), 1200),
                workspace_changed: false,
                exit_code: None,
                error_code: None,
                planned_layer: layer_idx_by_id.get(&n.id).copied(),
            })
            .collect();

        let topological_order: Vec<String> = execution_layers
            .iter()
            .flat_map(|layer| layer.iter().cloned())
            .collect();

        let report = WorkflowExecutionReport {
            report_type: "workflow_validate_result".to_string(),
            status: "planned".to_string(),
            workspace_changed: false,
            spec: serde_json::json!({
                "max_parallelism": spec.max_parallelism,
                "fail_fast": spec.fail_fast,
                "compensate_on_failure": spec.compensate_on_failure,
                "output_inject_max_chars": spec.output_inject_max_chars,
                "nodes_count": spec.nodes.len(),
                "execution_layers": execution_layers,
                "layer_count": execution_layers.len(),
                "topological_order": topological_order
            }),
            stats: WorkflowExecutionStats {
                passed: 0,
                failed: 0,
                skipped: 0,
            },
            nodes: node_reports,
            first_failure: None,
            compensation: WorkflowExecutionCompensationReport {
                executed: false,
                summary: None,
            },
            human_summary: format!(
                "workflow_validate_only: DAG 校验通过，已生成规划（planned nodes={}，layers={}）",
                spec.nodes.len(),
                execution_layers.len()
            ),
        };

        info!(
            workflow_run_id = workflow_run_id,
            nodes_count = spec.nodes.len(),
            layer_count = execution_layers.len(),
            "workflow_validate_only planned"
        );
        let json = serde_json::to_string(&report).unwrap_or_else(|_| report.human_summary.clone());
        return (json, false);
    }

    let spec = match parse_workflow_spec(args_json) {
        Ok(s) => s,
        Err(e) => {
            warn!(workflow_run_id = workflow_run_id, error = %e, "workflow_execute parse spec failed");
            let report = serde_json::json!({
                "type": "workflow_execute_error",
                "status": "failed",
                "workspace_changed": false,
                "human_summary": format!("workflow_execute 参数解析错误：{}", e)
            });
            return (report.to_string(), false);
        }
    };

    if let Err(e) = validate_dag(&spec.nodes) {
        warn!(workflow_run_id = workflow_run_id, error = %e, "workflow_execute dag validation failed");
        let report = serde_json::json!({
            "type": "workflow_execute_error",
            "status": "failed",
            "workspace_changed": false,
            "human_summary": format!("workflow_execute workflow 校验失败：{}", e)
        });
        return (report.to_string(), false);
    }

    let approval_mode = approval_mode;
    let workdir = effective_working_dir.to_path_buf();
    let allowed_commands = cfg.allowed_commands.clone();
    let weather_timeout_secs = cfg.weather_timeout_secs;
    let command_timeout_secs = cfg.command_timeout_secs;
    let web_search_timeout_secs = cfg.web_search_timeout_secs;
    let web_search_provider = cfg.web_search_provider;
    let web_search_api_key = cfg.web_search_api_key.clone();
    let web_search_max_results = cfg.web_search_max_results;
    let http_fetch_timeout_secs = cfg.http_fetch_timeout_secs;
    let http_fetch_max_response_bytes = cfg.http_fetch_max_response_bytes;
    let http_fetch_allowed_prefixes = cfg.http_fetch_allowed_prefixes.clone();

    let tool_exec_ctx = WorkflowToolExecCtx {
        cfg_command_timeout_secs: command_timeout_secs,
        cfg_weather_timeout_secs: weather_timeout_secs,
        cfg_web_search_timeout_secs: web_search_timeout_secs,
        cfg_web_search_provider: web_search_provider,
        cfg_web_search_api_key: web_search_api_key,
        cfg_web_search_max_results: web_search_max_results,
        cfg_http_fetch_timeout_secs: http_fetch_timeout_secs,
        cfg_http_fetch_max_response_bytes: http_fetch_max_response_bytes,
        cfg_http_fetch_allowed_prefixes: http_fetch_allowed_prefixes,
        cfg_allowed_commands: allowed_commands,
        effective_working_dir: workdir,
        workspace_is_set,
        command_max_output_len,
        workflow_run_id,
    };

    let (main_result, workspace_changed) =
        execute_workflow_dag(spec, approval_mode, tool_exec_ctx).await;
    info!(
        workflow_run_id = workflow_run_id,
        workspace_changed = workspace_changed,
        "workflow_execute finished"
    );
    (main_result, workspace_changed)
}

fn topo_layers(nodes: &[WorkflowNodeSpec]) -> Result<Vec<Vec<String>>, String> {
    // Kahn 算法逐层生成拓扑层级。
    let mut indegree: HashMap<String, usize> =
        nodes.iter().map(|n| (n.id.clone(), 0usize)).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for n in nodes.iter() {
        for d in n.deps.iter() {
            adj.entry(d.clone()).or_default().push(n.id.clone());
            *indegree
                .get_mut(&n.id)
                .ok_or("internal error: missing indegree")? += 1;
        }
    }

    let mut current: VecDeque<String> = indegree
        .iter()
        .filter_map(|(k, v)| if *v == 0 { Some(k.clone()) } else { None })
        .collect();
    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut visited = 0usize;

    while !current.is_empty() {
        let layer_nodes: Vec<String> = current.into_iter().collect();
        let mut next: VecDeque<String> = VecDeque::new();

        for x in layer_nodes.iter() {
            visited += 1;
            if let Some(ns) = adj.get(x) {
                for y in ns.iter() {
                    let entry = indegree
                        .get_mut(y)
                        .ok_or("internal error: missing indegree node")?;
                    *entry -= 1;
                    if *entry == 0 {
                        next.push_back(y.clone());
                    }
                }
            }
        }

        layers.push(layer_nodes);
        current = next;
    }

    if visited != nodes.len() {
        return Err("workflow_validate_only: 存在循环依赖（DAG 层级计算失败）".to_string());
    }
    Ok(layers)
}

#[derive(Debug, Clone)]
struct WorkflowToolExecCtx {
    cfg_command_timeout_secs: u64,
    cfg_weather_timeout_secs: u64,
    cfg_web_search_timeout_secs: u64,
    cfg_web_search_provider: crate::config::WebSearchProvider,
    cfg_web_search_api_key: String,
    cfg_web_search_max_results: u32,
    cfg_http_fetch_timeout_secs: u64,
    cfg_http_fetch_max_response_bytes: usize,
    cfg_http_fetch_allowed_prefixes: Vec<String>,
    cfg_allowed_commands: Vec<String>,
    effective_working_dir: PathBuf,
    workspace_is_set: bool,
    command_max_output_len: usize,
    workflow_run_id: u64,
}

async fn execute_workflow_dag(
    spec: WorkflowSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
) -> (String, bool) {
    let workflow_run_id = tool_exec_ctx.workflow_run_id;
    info!(
        workflow_run_id = workflow_run_id,
        nodes_count = spec.nodes.len(),
        max_parallelism = spec.max_parallelism,
        fail_fast = spec.fail_fast,
        compensate_on_failure = spec.compensate_on_failure,
        "workflow dag execute start"
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
                                    output: "workflow 并发控制异常（semaphore closed）".to_string(),
                                    workspace_changed: false,
                                    exit_code: None,
                                    error_code: Some("workflow_semaphore_closed".to_string()),
                                };
                            }
                        };
                        run_node(
                            node_cloned,
                            approval_mode_cloned,
                            exec_ctx,
                            completed_snapshot,
                            inject_max_chars,
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
                output_preview: truncate_for_summary(&r.output, 1200),
                workspace_changed: r.workspace_changed,
                exit_code: r.exit_code,
                error_code: r.error_code.clone(),
                planned_layer: None,
            });
        } else if started.contains(&n.id) {
            // 理论上不会发生：started 了但没有输出结果
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

    let report = WorkflowExecutionReport {
        report_type: "workflow_execute_result".to_string(),
        status,
        workspace_changed: workspace_changed_final,
        spec: serde_json::json!({
            "max_parallelism": spec.max_parallelism,
            "fail_fast": spec.fail_fast,
            "compensate_on_failure": spec.compensate_on_failure,
            "output_inject_max_chars": spec.output_inject_max_chars,
                "nodes_count": spec.nodes.len(),
                "planned_layer_count": 0
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
        human_summary,
    };

    let json = serde_json::to_string(&report).unwrap_or_else(|_| report.human_summary.clone());
    info!(
        workflow_run_id = workflow_run_id,
        status = %report.status,
        passed = passed,
        failed = failed,
        skipped = skipped,
        workspace_changed = workspace_changed_final,
        "workflow dag execute finished"
    );
    (json, workspace_changed)
}

fn command_max_output_len_from(ctx: &WorkflowToolExecCtx) -> usize {
    ctx.command_max_output_len
}

fn node_ready(deps: &[String], completed: &HashMap<String, NodeRunResult>) -> bool {
    deps.iter().all(|d| completed.contains_key(d))
}

async fn run_node(
    node: WorkflowNodeSpec,
    approval_mode: WorkflowApprovalMode,
    tool_exec_ctx: WorkflowToolExecCtx,
    completed_snapshot: HashMap<String, NodeRunResult>,
    inject_max_chars: usize,
) -> NodeRunResult {
    let node_start = Instant::now();
    info!(
        workflow_run_id = tool_exec_ctx.workflow_run_id,
        node_id = %node.id,
        tool_name = %node.tool_name,
        "workflow node start"
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
    let mut effective_allowed = tool_exec_ctx.cfg_allowed_commands.clone();
    let mut workspace_changed = false;

    // workspace_is_set 校验（主要覆盖 run_command/run_executable）
    if !tool_exec_ctx.workspace_is_set
        && (node.tool_name == "run_command" || node.tool_name == "run_executable")
    {
        return NodeRunResult {
            id: node.id,
            status: NodeRunStatus::Failed,
            output:
                "错误：未设置工作区，禁止在工作流中执行该工具（需要 TUI/CLI 先设置 workspace）。"
                    .to_string(),
            workspace_changed: false,
            exit_code: None,
            error_code: Some("workspace_not_set".to_string()),
        };
    }

    // run_command 特殊：按 cmd 白名单 + persistent allowlist 审批
    if node.tool_name == "run_command" {
        if let Some(cmd) = node.tool_args.get("command").and_then(|x| x.as_str()) {
            let cmd_lower = cmd.trim().to_lowercase();
            let disallowed = !tool_exec_ctx
                .cfg_allowed_commands
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&cmd_lower));

            let already_allowed = match &approval_mode {
                WorkflowApprovalMode::Tui {
                    persistent_allowlist,
                    ..
                } => {
                    let guard = persistent_allowlist.lock().await;
                    guard.contains(&cmd_lower)
                }
                WorkflowApprovalMode::NoApproval => false,
            };

            if disallowed && !already_allowed && !cmd_lower.is_empty() {
                // 仅在 TUI 审批模式下才能等待用户决策
                let decision = match &approval_mode {
                    WorkflowApprovalMode::Tui {
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
                            ),
                            workspace_changed: false,
                            exit_code: None,
                            error_code: Some("command_not_allowed".to_string()),
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
                            ),
                            workspace_changed: false,
                            exit_code: None,
                            error_code: Some("command_denied".to_string()),
                        };
                    }
                    CommandApprovalDecision::AllowOnce => {
                        effective_allowed.push(cmd_lower.clone());
                    }
                    CommandApprovalDecision::AllowAlways => {
                        effective_allowed.push(cmd_lower.clone());
                        if let WorkflowApprovalMode::Tui {
                            persistent_allowlist,
                            ..
                        } = &approval_mode
                        {
                            persistent_allowlist.lock().await.insert(cmd_lower.clone());
                        }
                    }
                }
            }
        }
    } else if node.requires_approval {
        // 通用人工审批节点：仅 TUI 模式支持
        let approval_key = format!("workflow_node:{}", node.id).to_lowercase();

        match approval_mode {
            WorkflowApprovalMode::NoApproval => {
                return NodeRunResult {
                    id: node.id,
                    status: NodeRunStatus::Failed,
                    output: format!(
                        "workflow 执行失败：该节点需要人工审批，但当前不在 TUI 模式：{}",
                        approval_key
                    ),
                    workspace_changed: false,
                    exit_code: None,
                    error_code: Some("approval_required".to_string()),
                };
            }
            WorkflowApprovalMode::Tui {
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
                                ),
                                workspace_changed: false,
                                exit_code: None,
                                error_code: Some("approval_denied".to_string()),
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
        "get_weather" => Some(tool_exec_ctx.cfg_weather_timeout_secs),
        "web_search" => Some(tool_exec_ctx.cfg_web_search_timeout_secs),
        "http_fetch" => Some(
            tool_exec_ctx
                .cfg_http_fetch_timeout_secs
                .max(tool_exec_ctx.cfg_command_timeout_secs),
        ),
        _ => None,
    });

    let tool_name = node.tool_name.clone();
    let exec_args = tool_args_json_str.clone();
    let exec_args_for_success = exec_args.clone();
    let run_command_working_dir = tool_exec_ctx.effective_working_dir.clone();
    let allowed_slice = effective_allowed.clone();
    let command_max_output_len = tool_exec_ctx.command_max_output_len;
    let weather_timeout_secs = tool_exec_ctx.cfg_weather_timeout_secs;
    let ws_timeout = tool_exec_ctx.cfg_web_search_timeout_secs;
    let ws_provider = tool_exec_ctx.cfg_web_search_provider;
    let ws_max = tool_exec_ctx.cfg_web_search_max_results;
    let ws_key = tool_exec_ctx.cfg_web_search_api_key.clone();
    let hf_pfx = tool_exec_ctx.cfg_http_fetch_allowed_prefixes.clone();
    let hf_to = tool_exec_ctx.cfg_http_fetch_timeout_secs;
    let hf_mb = tool_exec_ctx.cfg_http_fetch_max_response_bytes;

    let output_res = async move {
        let work_dir = run_command_working_dir;
        let allowed = allowed_slice;
        let handle = tokio::task::spawn_blocking(move || {
            let ctx = crate::tools::ToolContext {
                command_max_output_len,
                weather_timeout_secs,
                allowed_commands: &allowed,
                working_dir: &work_dir,
                web_search_timeout_secs: ws_timeout,
                web_search_provider: ws_provider,
                web_search_api_key: ws_key.as_str(),
                web_search_max_results: ws_max,
                http_fetch_allowed_prefixes: hf_pfx.as_slice(),
                http_fetch_timeout_secs: hf_to,
                http_fetch_max_response_bytes: hf_mb,
            };
            crate::tools::run_tool_result(&tool_name, &exec_args, &ctx)
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
                    id: node.id,
                    status: NodeRunStatus::Failed,
                    output: format!("workflow 节点超时（{} 秒）：tool={}", ts, node.tool_name),
                    workspace_changed: false,
                    exit_code: None,
                    error_code: Some("timeout".to_string()),
                };
            }
        }
    } else {
        output_res.await
    };

    if node.tool_name == "run_command"
        && crate::tools::is_compile_command_success(&exec_args_for_success, &tool_result.message)
    {
        workspace_changed = true;
    }

    let status = if tool_result.ok {
        NodeRunStatus::Passed
    } else {
        NodeRunStatus::Failed
    };
    let output = tool_result.message.clone();
    let result = NodeRunResult {
        id: node.id,
        status,
        output,
        workspace_changed,
        exit_code: tool_result.exit_code,
        error_code: tool_result.error_code.clone(),
    };
    info!(
        workflow_run_id = tool_exec_ctx.workflow_run_id,
        node_id = %result.id,
        tool_name = %node.tool_name,
        status = ?result.status,
        elapsed_ms = node_start.elapsed().as_millis(),
        exit_code = result.exit_code,
        error_code = ?result.error_code,
        stdout_len = tool_result.stdout.len(),
        stderr_len = tool_result.stderr.len(),
        "workflow node finished"
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
    // 保证同一时间只有一个审批请求处于“发送 -> 等待决策”的进行中，避免并发覆盖 TUI 状态。
    let _guard = approval_request_guard.lock().await;
    let line =
        crate::sse_protocol::encode_message(crate::sse_protocol::SsePayload::CommandApproval {
            command_approval_request: crate::sse_protocol::CommandApprovalBody {
                command: command.to_string(),
                args: args.to_string(),
                allowlist_key: None,
            },
        });
    let _ = out_tx.send(line).await;

    let mut rx_guard = approval_rx.lock().await;
    rx_guard
        .recv()
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
                truncate_for_summary(&r.output, 1200)
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
                truncate_for_summary(&r.output, 1200)
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

fn truncate_for_summary(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}... (截断)", &s[..max_chars])
    }
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
                truncate_for_summary(&res.output, 800)
            ));
        }
    }

    if any_failed {
        out.push_str("\n补偿执行存在失败（需要人工介入确认一致性）。");
    }
    (out, any_workspace_changed)
}

fn validate_dag(nodes: &[WorkflowNodeSpec]) -> Result<(), String> {
    let mut node_map: HashMap<&str, &WorkflowNodeSpec> = HashMap::new();
    for n in nodes.iter() {
        node_map.insert(&n.id, n);
    }
    for n in nodes.iter() {
        for d in n.deps.iter() {
            if !node_map.contains_key(d.as_str()) {
                return Err(format!("节点 {} 依赖了未知节点 {}", n.id, d));
            }
        }
    }
    // cycle detection: Kahn
    let mut indegree: HashMap<String, usize> = nodes.iter().map(|n| (n.id.clone(), 0)).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for n in nodes.iter() {
        for d in n.deps.iter() {
            indegree.entry(n.id.clone()).and_modify(|x| *x += 1);
            adj.entry(d.clone()).or_default().push(n.id.clone());
        }
    }

    let mut q = VecDeque::new();
    for (k, v) in indegree.iter() {
        if *v == 0 {
            q.push_back(k.clone());
        }
    }
    let mut visited = 0usize;
    while let Some(x) = q.pop_front() {
        visited += 1;
        if let Some(next) = adj.get(&x) {
            for y in next.iter() {
                if let Some(v) = indegree.get_mut(y) {
                    *v -= 1;
                    if *v == 0 {
                        q.push_back(y.clone());
                    }
                }
            }
        }
    }
    if visited != nodes.len() {
        return Err("workflow 存在循环依赖（DAG 校验失败）".to_string());
    }
    Ok(())
}

fn parse_workflow_spec(args_json: &str) -> Result<WorkflowSpec, String> {
    let v: serde_json::Value = serde_json::from_str(args_json).map_err(|e| e.to_string())?;
    let spec_v = v.get("workflow").unwrap_or(&v);

    let max_parallelism = spec_v
        .get("max_parallelism")
        .and_then(|x| x.as_u64())
        .unwrap_or(4) as usize;
    let fail_fast = spec_v
        .get("fail_fast")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let compensate_on_failure = spec_v
        .get("compensate_on_failure")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);

    let output_inject_max_chars = spec_v
        .get("output_inject_max_chars")
        .and_then(|x| x.as_u64())
        .unwrap_or(2000) as usize;

    let nodes_v = spec_v.get("nodes").ok_or("workflow 缺少 nodes 字段")?;
    let mut nodes: Vec<WorkflowNodeSpec> = Vec::new();

    if let Some(arr) = nodes_v.as_array() {
        for it in arr.iter() {
            nodes.push(parse_node_from_value(it, None)?);
        }
    } else if nodes_v.is_object() {
        let obj = nodes_v
            .as_object()
            .ok_or_else(|| "workflow.nodes 不是对象".to_string())?;
        for (id, it) in obj.iter() {
            nodes.push(parse_node_from_value(it, Some(id))?);
        }
    } else {
        return Err("workflow.nodes 必须是数组或对象".to_string());
    }

    if nodes.is_empty() {
        return Err("workflow.nodes 不能为空".to_string());
    }
    Ok(WorkflowSpec {
        max_parallelism,
        fail_fast,
        compensate_on_failure,
        output_inject_max_chars,
        nodes,
    })
}

fn parse_node_from_value(
    v: &serde_json::Value,
    forced_id: Option<&String>,
) -> Result<WorkflowNodeSpec, String> {
    let id = forced_id
        .cloned()
        .or_else(|| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .ok_or("node 缺少 id")?;

    let tool_name = v
        .get("tool_name")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("tool").and_then(|x| x.as_str()))
        .ok_or(format!("node {} 缺少 tool_name", id))?
        .to_string();

    let tool_args = v
        .get("tool_args")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let deps = v
        .get("deps")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let requires_approval = v
        .get("requires_approval")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let timeout_secs = v.get("timeout_secs").and_then(|x| x.as_u64());

    let compensate_with = v
        .get("compensate_with")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(WorkflowNodeSpec {
        id,
        tool_name,
        tool_args,
        deps,
        requires_approval,
        timeout_secs,
        compensate_with,
    })
}

// (tool_args_to_string 已不再需要：运行时会对 tool_args 做占位符注入后再序列化)

fn inject_placeholders(
    value: &serde_json::Value,
    completed: &HashMap<String, NodeRunResult>,
    max_chars: usize,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            serde_json::Value::String(inject_string(s, completed, max_chars))
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| inject_placeholders(v, completed, max_chars))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), inject_placeholders(v, completed, max_chars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn inject_string(s: &str, completed: &HashMap<String, NodeRunResult>, max_chars: usize) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        let start = match rest.find("{{") {
            Some(i) => i,
            None => {
                out.push_str(rest);
                break;
            }
        };
        let (prefix, tail) = rest.split_at(start);
        out.push_str(prefix);
        let end = match tail.find("}}") {
            Some(i) => i,
            None => {
                // 没有闭合，直接把剩余内容追加
                out.push_str(tail);
                break;
            }
        };
        let inner = tail[2..end].trim(); // skip {{
        let replacement = resolve_placeholder(inner, completed, max_chars);
        out.push_str(&replacement);
        // move past }}
        rest = &tail[end + 2..];
    }
    out
}

fn resolve_placeholder(
    inner: &str,
    completed: &HashMap<String, NodeRunResult>,
    max_chars: usize,
) -> String {
    // 支持：
    // - {{node_id.output}}
    // - {{node_id.status}}
    // - {{node_id.stdout_first_line}}
    // 未来可扩展更多字段。
    let parts: Vec<&str> = inner.split('.').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return String::new();
    }

    let node_id = parts[0];
    if let Some(r) = completed.get(node_id) {
        let field = if parts.len() == 2 { parts[1] } else { parts[2] };
        match field {
            "output" => truncate_for_injection(&r.output, max_chars),
            "status" => match r.status {
                NodeRunStatus::Passed => "passed".to_string(),
                NodeRunStatus::Failed => "failed".to_string(),
            },
            "stdout_first_line" => r
                .output
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .chars()
                .take(max_chars)
                .collect::<String>(),
            "stdout_first_token" => r
                .output
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .take(max_chars)
                .collect::<String>(),
            _ => String::new(),
        }
    } else {
        String::new()
    }
}

fn truncate_for_injection(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}... (截断)", &s[..max_chars])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workflow_spec_array_nodes() {
        let json = r#"{
            "workflow":{
              "max_parallelism":2,
              "fail_fast":true,
              "compensate_on_failure":true,
              "nodes":[
                {"id":"a","tool_name":"get_current_time","tool_args":{},"deps":[]},
                {"id":"b","tool_name":"calc","tool_args":{"expression":"1+1"},"deps":["a"]}
              ]
            }
        }"#;
        let spec = parse_workflow_spec(json).unwrap();
        assert_eq!(spec.nodes.len(), 2);
        assert_eq!(spec.nodes[0].id, "a");
        assert_eq!(spec.nodes[1].deps, vec!["a".to_string()]);
    }

    #[test]
    fn test_validate_dag_cycle_detection() {
        let nodes = vec![
            WorkflowNodeSpec {
                id: "a".to_string(),
                tool_name: "calc".to_string(),
                tool_args: serde_json::json!({}),
                deps: vec!["b".to_string()],
                requires_approval: false,
                timeout_secs: None,
                compensate_with: vec![],
            },
            WorkflowNodeSpec {
                id: "b".to_string(),
                tool_name: "calc".to_string(),
                tool_args: serde_json::json!({}),
                deps: vec!["a".to_string()],
                requires_approval: false,
                timeout_secs: None,
                compensate_with: vec![],
            },
        ];
        assert!(validate_dag(&nodes).is_err());
    }

    #[test]
    fn test_node_ready() {
        let completed: HashMap<String, NodeRunResult> = HashMap::new();
        let ready = node_ready(&[] as &[String], &completed);
        assert!(ready);
    }

    #[test]
    fn test_inject_placeholders_output_truncation() {
        let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
        completed.insert(
            "a".to_string(),
            NodeRunResult {
                id: "a".to_string(),
                status: NodeRunStatus::Passed,
                output: "hello world".repeat(200),
                workspace_changed: false,
                exit_code: Some(0),
                error_code: None,
            },
        );
        let v = serde_json::json!({"x":"prefix {{a.output}} suffix"});
        let injected = inject_placeholders(&v, &completed, 20);
        let x = injected.get("x").and_then(|y| y.as_str()).unwrap_or("");
        assert!(x.contains("prefix "));
        assert!(x.contains("suffix"));
        assert!(x.len() <= "prefix ".len() + 20 + " suffix".len() + 32); // 允许截断标记冗余
    }

    #[test]
    fn test_placeholder_stdout_first_token() {
        let mut completed: HashMap<String, NodeRunResult> = HashMap::new();
        completed.insert(
            "a".to_string(),
            NodeRunResult {
                id: "a".to_string(),
                status: NodeRunStatus::Passed,
                output: "deadbeef123 some message\nsecond line".to_string(),
                workspace_changed: false,
                exit_code: Some(0),
                error_code: None,
            },
        );
        let v = serde_json::json!({"rev":"{{a.stdout_first_token}}"});
        let injected = inject_placeholders(&v, &completed, 64);
        let rev = injected.get("rev").and_then(|x| x.as_str()).unwrap_or("");
        assert_eq!(rev, "deadbeef123");
    }
}
