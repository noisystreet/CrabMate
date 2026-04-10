//! 单节点执行：占位符注入、审批门闩、`run_tool`、按工具类型解析 SLA 超时、失败退避重试。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use log::info;
use tokio::sync::{Mutex, mpsc};

use crate::types::CommandApprovalDecision;

use super::super::model::WorkflowNodeSpec;
use super::super::placeholders::inject_placeholders;
use super::super::types::{NodeRunResult, NodeRunStatus};
use super::retry::workflow_node_failure_retryable;
use super::trace::{WorkflowTracePush, workflow_trace_push};
use super::{WorkflowApprovalMode, WorkflowToolExecCtx};

pub(super) fn command_max_output_len_from(ctx: &WorkflowToolExecCtx) -> usize {
    ctx.command_max_output_len
}

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
    let command_timeout_secs = tool_exec_ctx.cfg_command_timeout_secs;

    let output_res = async move {
        let work_dir = run_command_working_dir;
        let allowed = effective_allowed_arc;
        let handle = tokio::task::spawn_blocking(move || {
            let ctx = crate::tools::ToolContext {
                cfg: None,
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
                command_timeout_secs,
                read_file_turn_cache: None,
                workspace_changelist: None,
                test_result_cache_enabled,
                test_result_cache_max_entries,
                long_term_memory: None,
                long_term_memory_scope_id: None,
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

pub(crate) async fn run_node(
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

fn workflow_node_workspace_failure_if_unset(
    node: &WorkflowNodeSpec,
    tool_exec_ctx: &WorkflowToolExecCtx,
) -> Option<NodeRunResult> {
    if tool_exec_ctx.workspace_is_set {
        return None;
    }
    if node.tool_name != "run_command" && node.tool_name != "run_executable" {
        return None;
    }
    Some(NodeRunResult {
        id: node.id.clone(),
        status: NodeRunStatus::Failed,
        output: "错误：未设置工作区，禁止在工作流中执行该工具（需要先在 CLI/Web 设置 workspace）。"
            .into(),
        workspace_changed: false,
        exit_code: None,
        error_code: Some("workspace_not_set".to_string()),
        attempt: 1,
    })
}

/// `run_command`：白名单扩展 + 交互审批；其它工具类型直接返回配置白名单。
async fn apply_run_command_allowlist_approvals(
    node: &WorkflowNodeSpec,
    approval_mode: &WorkflowApprovalMode,
    tool_exec_ctx: &WorkflowToolExecCtx,
) -> Result<Arc<[String]>, NodeRunResult> {
    let mut effective_allowed_arc: Arc<[String]> = Arc::clone(&tool_exec_ctx.cfg_allowed_commands);
    if node.tool_name != "run_command" {
        return Ok(effective_allowed_arc);
    }
    let Some(cmd) = node.tool_args.get("command").and_then(|x| x.as_str()) else {
        return Ok(effective_allowed_arc);
    };
    let cmd_lower = cmd.trim().to_lowercase();
    let disallowed = !tool_exec_ctx
        .cfg_allowed_commands
        .as_ref()
        .iter()
        .any(|c| c.eq_ignore_ascii_case(&cmd_lower));

    let already_allowed = match approval_mode {
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
        let mut v: Vec<String> = tool_exec_ctx.cfg_allowed_commands.iter().cloned().collect();
        v.push(cmd_lower.clone());
        effective_allowed_arc = v.into();
    }

    if disallowed && !already_allowed && !cmd_lower.is_empty() {
        let decision = match approval_mode {
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
                return Err(NodeRunResult {
                    id: node.id.clone(),
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
                });
            }
        };

        match decision {
            CommandApprovalDecision::Deny => {
                return Err(NodeRunResult {
                    id: node.id.clone(),
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
                });
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
                } = approval_mode
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

    Ok(effective_allowed_arc)
}

/// 非 `run_command` 且 `requires_approval` 时的通用人工审批门闩。
async fn apply_generic_workflow_node_approval(
    node: &WorkflowNodeSpec,
    approval_mode: &WorkflowApprovalMode,
) -> Result<(), NodeRunResult> {
    if !node.requires_approval || node.tool_name == "run_command" {
        return Ok(());
    }
    let approval_key = format!("workflow_node:{}", node.id).to_lowercase();

    match approval_mode {
        WorkflowApprovalMode::NoApproval => {
            return Err(NodeRunResult {
                id: node.id.clone(),
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
            });
        }
        WorkflowApprovalMode::Interactive {
            out_tx,
            approval_rx,
            approval_request_guard,
            persistent_allowlist,
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
                        return Err(NodeRunResult {
                            id: node.id.clone(),
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
                        });
                    }
                    CommandApprovalDecision::AllowOnce => {}
                    CommandApprovalDecision::AllowAlways => {
                        persistent_allowlist.lock().await.insert(approval_key);
                    }
                }
            }
        }
    }
    Ok(())
}

fn resolve_workflow_node_timeout_secs(
    node: &WorkflowNodeSpec,
    tool_exec_ctx: &WorkflowToolExecCtx,
) -> Option<u64> {
    node.timeout_secs.or(match node.tool_name.as_str() {
        "run_command" | "run_executable" | "python_snippet_run" => {
            Some(tool_exec_ctx.cfg_command_timeout_secs)
        }
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
    })
}

/// 工具执行 + 可重试失败退避（timeout / join / semaphore 类）。
async fn run_workflow_node_tool_with_retries(
    node: &WorkflowNodeSpec,
    tool_args_json_str: &str,
    tool_exec_ctx: &WorkflowToolExecCtx,
    effective_allowed_arc: Arc<[String]>,
    timeout_secs: Option<u64>,
    phase: &'static str,
    node_start: Instant,
) -> NodeRunResult {
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
            tool_args_json_str,
            tool_exec_ctx,
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

    let injected_tool_args =
        inject_placeholders(&node.tool_args, &completed_snapshot, inject_max_chars);
    let tool_args_json_str = if injected_tool_args.is_null() {
        "{}".to_string()
    } else {
        injected_tool_args.to_string()
    };

    if let Some(fail) = workflow_node_workspace_failure_if_unset(&node, &tool_exec_ctx) {
        return fail;
    }

    let effective_allowed_arc =
        match apply_run_command_allowlist_approvals(&node, &approval_mode, &tool_exec_ctx).await {
            Ok(a) => a,
            Err(res) => return res,
        };

    if let Err(res) = apply_generic_workflow_node_approval(&node, &approval_mode).await {
        return res;
    }

    let timeout_secs = resolve_workflow_node_timeout_secs(&node, &tool_exec_ctx);

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

    run_workflow_node_tool_with_retries(
        &node,
        tool_args_json_str.as_str(),
        &tool_exec_ctx,
        effective_allowed_arc,
        timeout_secs,
        phase,
        node_start,
    )
    .await
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
