//! `dispatch_tool` 及各类需异步/阻塞池的工具执行实现。
//!
//! 进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]；白名单等同理。详见仓库 `tool_registry` 模块说明。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::error;
use tokio::sync::Mutex;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::workflow;
use crate::agent::workflow_reflection_controller;
use crate::config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::tools;
use crate::types::{CommandApprovalDecision, ToolCall};

use super::meta::{HandlerId, handler_id_for};
use super::policy::{
    http_fetch_outer_wall_secs, http_request_outer_wall_secs, parallel_tool_wall_timeout_secs,
    sync_default_runs_inline,
};
use super::runtime::{CliToolRuntime, ToolRuntime, WebToolRuntime};

/// Web UI：未选择工作区时的统一提示尾句（`run_command` / `run_executable` 共用）。
const WEB_WORKSPACE_PANEL_HINT: &str = "请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。";

fn web_tool_err_workspace_not_set(action_zh: &str) -> String {
    format!("错误：未设置工作区，禁止{action_zh}。{WEB_WORKSPACE_PANEL_HINT}")
}

/// 在配置白名单基础上追加一条命令名（`run_command` 审批通过路径共用）。
fn extend_allowed_commands_arc(
    base: &std::sync::Arc<[String]>,
    cmd: &str,
) -> std::sync::Arc<[String]> {
    let mut v: Vec<String> = base.iter().cloned().collect();
    v.push(cmd.to_string());
    v.into()
}
pub struct DispatchToolParams<'a> {
    pub runtime: ToolRuntime<'a>,
    pub per_coord: &'a mut PerCoordinator,
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub name: &'a str,
    pub args: &'a str,
    pub tc: &'a ToolCall,
    pub read_file_turn_cache:
        Option<std::sync::Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    pub workspace_changelist:
        Option<std::sync::Arc<crate::workspace_changelist::WorkspaceChangelist>>,
    pub mcp_session: Option<&'a Arc<Mutex<crate::mcp::McpClientSession>>>,
    /// 并入整请求 `turn-*.json` 时传给 `workflow_execute`。
    pub request_chrome_merge: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
}

/// `http_fetch` / `http_request` 共用：`Web` 带可选审批会话，`Cli` 带终端审批上下文（本路径不使用 `workspace_changed`）。
fn http_tool_approval_context<'a>(
    runtime: ToolRuntime<'a>,
) -> (Option<&'a WebToolRuntime>, Option<&'a CliToolRuntime>) {
    match runtime {
        ToolRuntime::Web { ctx, .. } => (ctx, None),
        ToolRuntime::Cli { ctx, .. } => (None, Some(ctx)),
    }
}

/// Web / CLI 统一入口：`(tool_result_text, workflow 反思注入)`。
pub async fn dispatch_tool(p: DispatchToolParams<'_>) -> (String, Option<serde_json::Value>) {
    let DispatchToolParams {
        runtime,
        per_coord,
        cfg,
        effective_working_dir,
        workspace_is_set,
        name,
        args,
        tc,
        read_file_turn_cache,
        workspace_changelist,
        mcp_session,
        request_chrome_merge,
    } = p;
    if crate::mcp::is_mcp_proxy_tool(name) {
        let Some(remote) = crate::mcp::try_mcp_tool_name(cfg.as_ref(), name) else {
            return (
                "错误：无法将工具名解析为 MCP 远端名（请检查 mcp_command 与命名前缀）".to_string(),
                None,
            );
        };
        let Some(sess) = mcp_session else {
            return (
                "错误：MCP 会话未建立（连接或 tools/list 失败）".to_string(),
                None,
            );
        };
        let guard = sess.lock().await;
        let mcp_args = crate::tool_call_explain::strip_explain_why_if_present(args);
        let out = crate::mcp::call_mcp_tool(
            &guard,
            remote.as_str(),
            mcp_args.as_str(),
            Duration::from_secs(cfg.mcp_tool_timeout_secs.max(1)),
            cfg.command_max_output_len,
        )
        .await;
        return (out, None);
    }

    let args_processed =
        match crate::tool_call_explain::require_explain_for_mutation(cfg.as_ref(), name, args) {
            Ok(c) => c,
            Err(e) => return (e, None),
        };
    let args = args_processed.as_ref();

    let hid = handler_id_for(name);

    match hid {
        HandlerId::Workflow => {
            let runtime_web = match runtime {
                ToolRuntime::Web {
                    workspace_changed,
                    ctx,
                } => ToolRuntime::Web {
                    workspace_changed,
                    ctx,
                },
                ToolRuntime::Cli {
                    workspace_changed, ..
                } => ToolRuntime::Web {
                    workspace_changed,
                    ctx: None,
                },
            };
            execute_workflow(
                runtime_web,
                per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                args,
                request_chrome_merge,
            )
            .await
        }
        HandlerId::RunCommand => match runtime {
            ToolRuntime::Web {
                workspace_changed,
                ctx,
            } => {
                execute_run_command_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
                    ctx,
                    None,
                    name,
                    args,
                )
                .await
            }
            ToolRuntime::Cli {
                workspace_changed,
                ctx,
            } => {
                execute_run_command_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
                    None,
                    Some(ctx),
                    name,
                    args,
                )
                .await
            }
        },
        HandlerId::RunExecutable => {
            execute_run_executable_web(cfg, effective_working_dir, workspace_is_set, name, args)
                .await
        }
        HandlerId::GetWeather => {
            execute_get_weather_web(cfg, effective_working_dir, workspace_is_set, name, args).await
        }
        HandlerId::WebSearch => {
            execute_web_search_web(cfg, effective_working_dir, workspace_is_set, name, args).await
        }
        HandlerId::HttpFetch => {
            let (web_ctx, cli_ctx) = http_tool_approval_context(runtime);
            execute_http_fetch_impl(
                cfg,
                effective_working_dir,
                workspace_is_set,
                web_ctx,
                cli_ctx,
                name,
                args,
            )
            .await
        }
        HandlerId::HttpRequest => {
            let (web_ctx, cli_ctx) = http_tool_approval_context(runtime);
            execute_http_request_impl(
                cfg,
                effective_working_dir,
                workspace_is_set,
                web_ctx,
                cli_ctx,
                name,
                args,
            )
            .await
        }
        HandlerId::SyncDefault => {
            if cfg.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
                if !workspace_is_set {
                    return (
                        "错误：未设置工作区，无法在 Docker 沙盒中执行 SyncDefault 工具（请先设置工作区目录）。"
                            .to_string(),
                        None,
                    );
                }
                let out = crate::tool_sandbox::run_sync_default_in_docker(
                    cfg.as_ref(),
                    effective_working_dir,
                    name,
                    args,
                )
                .await;
                return match out {
                    Ok(s) => (s, None),
                    Err(e) => (e, None),
                };
            }
            if sync_default_runs_inline(cfg.as_ref(), name) {
                let ctx = tools::tool_context_for_with_read_cache(
                    cfg.as_ref(),
                    cfg.allowed_commands.as_ref(),
                    effective_working_dir,
                    read_file_turn_cache.as_ref().map(|a| a.as_ref()),
                    workspace_changelist.as_ref(),
                );
                return (tools::run_tool(name, args, &ctx), None);
            }
            let cfg2 = Arc::clone(cfg);
            let tool_name = tc.function.name.clone();
            let tool_args = tc.function.arguments.clone();
            let work_dir = effective_working_dir.to_path_buf();
            let rfc = read_file_turn_cache.clone();
            let wcl = workspace_changelist.clone();
            let wall_secs = parallel_tool_wall_timeout_secs(cfg.as_ref(), name);
            let handle = tokio::task::spawn_blocking(move || {
                let ctx = tools::tool_context_for_with_read_cache(
                    cfg2.as_ref(),
                    cfg2.allowed_commands.as_ref(),
                    work_dir.as_path(),
                    rfc.as_ref().map(|a| a.as_ref()),
                    wcl.as_ref(),
                );
                tools::run_tool(&tool_name, &tool_args, &ctx)
            });
            let result = match tokio::time::timeout(Duration::from_secs(wall_secs), handle).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    error!(
                        target: "crabmate",
                        "工具执行异常 tool={} error={:?}",
                        name,
                        e
                    );
                    format!("工具执行异常：{:?}", e)
                }
                Err(_) => {
                    error!(target: "crabmate", "工具执行超时 tool={} wall_secs={}", name, wall_secs);
                    format!("工具执行超时（{} 秒）", wall_secs)
                }
            };
            (result, None)
        }
    }
}

async fn execute_workflow(
    runtime: ToolRuntime<'_>,
    per_coord: &mut PerCoordinator,
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    args: &str,
    request_chrome_merge: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
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

/// `sync_default_tool_sandbox_mode = docker` 时，在宿主完成审批/白名单后把本类工具交给容器内 `tool-runner-internal`。
async fn dispatch_non_sync_tool_to_docker(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    kind: &str,
    args: &str,
    runner_cfg_path: Result<PathBuf, String>,
) -> Option<(String, Option<serde_json::Value>)> {
    if cfg.sync_default_tool_sandbox_mode != SyncDefaultToolSandboxMode::Docker {
        return None;
    }
    if !workspace_is_set {
        return Some((
            "错误：未设置工作区，无法在 Docker 沙盒中执行该工具（请先设置工作区目录）。"
                .to_string(),
            None,
        ));
    }
    let path = match runner_cfg_path {
        Ok(p) => p,
        Err(e) => return Some((e, None)),
    };
    let inv = crate::tool_sandbox::ToolInvocationLine {
        kind: kind.to_string(),
        tool: None,
        args_json: args.to_string(),
    };
    let out =
        crate::tool_sandbox::run_tool_in_docker(cfg.as_ref(), effective_working_dir, path, inv)
            .await;
    Some(match out {
        Ok(s) => (s, None),
        Err(e) => (e, None),
    })
}

#[allow(clippy::too_many_arguments)] // Web + CLI 双路径审批共享实现
async fn execute_run_command_impl(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    workspace_changed: &mut bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if !workspace_is_set {
        return (web_tool_err_workspace_not_set("执行命令"), None);
    }
    if let Some(ctx) = cli_ctx {
        ctx.record_run_command_attempt();
    }
    let v: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let cmd = v
        .get("command")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let arg_preview = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let base_allowed = Arc::clone(&cfg.allowed_commands);
    let mut effective_allowed_arc: Arc<[String]> = base_allowed;
    if !cmd.is_empty()
        && !effective_allowed_arc
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd))
    {
        let already_allowed = match (web_ctx, cli_ctx) {
            (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&cmd),
            (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&cmd),
            (None, None) => false,
        };
        if already_allowed {
            effective_allowed_arc = extend_allowed_commands_arc(&effective_allowed_arc, &cmd);
        } else {
            let allow_handles = crate::tool_approval::SharedAllowlistHandles {
                web: web_ctx.map(|w| &w.persistent_allowlist_shared),
                cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
            };
            let cmd_show = if arg_preview.is_empty() {
                cmd.clone()
            } else {
                format!("{} {}", cmd, arg_preview)
            };
            let spec = crate::tool_approval::ApprovalRequestSpec {
                capability: crate::tool_approval::SensitiveCapability::HostShell,
                sse_command: cmd.clone(),
                sse_args: arg_preview.clone(),
                allowlist_key: None,
                cli_title: "run_command 审批",
                cli_detail: format!("命令不在白名单:\n{}", cmd_show.trim()),
                web_timeline_prefix_zh: "命令审批：",
            };
            let decision_opt = if let Some(ctx) = cli_ctx {
                if ctx.auto_approve_all_non_whitelist_run_command
                    || ctx
                        .extra_allowlist_commands
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&cmd))
                {
                    Some(CommandApprovalDecision::AllowOnce)
                } else {
                    crate::tool_approval::request_tool_interactive_approval(
                        None,
                        Some(crate::tool_approval::CliApprovalInput {
                            auto_approve_all_sensitive: false,
                        }),
                        &spec,
                        "tool_registry::run_command approval",
                    )
                    .await
                    .ok()
                }
            } else if web_ctx.is_some() {
                match crate::tool_approval::request_tool_interactive_approval(
                    web_ctx.map(|w| w.approval_sink()),
                    None,
                    &spec,
                    "tool_registry::run_command approval",
                )
                .await
                {
                    Ok(d) => Some(d),
                    Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                        return ("错误：审批通道不可用，请重试。".to_string(), None);
                    }
                }
            } else {
                None
            };
            if let Some(decision) = decision_opt {
                match decision {
                    CommandApprovalDecision::Deny => {
                        if let Some(c) = cli_ctx {
                            c.record_run_command_denial();
                        }
                        return (format!("用户拒绝执行命令：{}", cmd_show.trim()), None);
                    }
                    CommandApprovalDecision::AllowOnce => {
                        effective_allowed_arc =
                            extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
                    }
                    CommandApprovalDecision::AllowAlways => {
                        crate::tool_approval::persist_allowlist_key(&allow_handles, &cmd).await;
                        effective_allowed_arc =
                            extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
                    }
                }
            }
        }
    }

    if let Some((s, inj)) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "run_command",
        args,
        crate::tool_sandbox::write_runner_config_json_with_allowed_commands(
            cfg.as_ref(),
            effective_allowed_arc.as_ref(),
        ),
    )
    .await
    {
        if tools::is_compile_command_success(args, &s) {
            *workspace_changed = true;
        }
        return (s, inj);
    }

    let name_in = name.to_string();
    let cmd_timeout = cfg.command_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_cloned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            effective_allowed_arc.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_cloned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(target: "crabmate", "命令执行超时 tool={}", name);
            format!("命令执行超时（{} 秒）", cmd_timeout)
        }
    };
    if tools::is_compile_command_success(args, &s) {
        *workspace_changed = true;
    }
    (s, None)
}

/// 并行只读批内 **`http_fetch`**：在 `spawn_blocking` 之前串行完成解析与白名单/审批，避免多请求竞态修改 `persistent_allowlist`。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或本就匹配前缀。
pub(crate) async fn prefetch_http_fetch_parallel_approvals(
    tool_calls: &[ToolCall],
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
) -> HashMap<(String, String), String> {
    let mut failures: HashMap<(String, String), String> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for tc in tool_calls {
        if tc.function.name != "http_fetch" {
            continue;
        }
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        let args = tc.function.arguments.as_str();
        let (url, method) = match tools::http_fetch::parse_http_fetch_args(args) {
            Ok(x) => x,
            Err(e) => {
                failures.insert(key, format!("错误：{}", e));
                continue;
            }
        };
        let storage_key = tools::http_fetch::storage_key(&url);
        let approval_args = tools::http_fetch::approval_args_display(method, &url);
        let allowed_by_cfg =
            tools::http_fetch::url_matches_allowed_prefixes(&url, &cfg.http_fetch_allowed_prefixes);
        let allowed_by_list = match (web_ctx, cli_ctx) {
            (Some(w), _) => w
                .persistent_allowlist_shared
                .lock()
                .await
                .contains(&storage_key),
            (None, Some(c)) => c
                .persistent_allowlist_shared
                .lock()
                .await
                .contains(&storage_key),
            (None, None) => false,
        };
        if allowed_by_cfg || allowed_by_list {
            continue;
        }
        if web_ctx.is_none() && cli_ctx.is_none() {
            failures.insert(
                key,
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
            );
            continue;
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpRead,
            sse_command: "http_fetch".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(storage_key.clone()),
            cli_title: "http_fetch 审批",
            cli_detail: format!(
                "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                approval_args
            ),
            web_timeline_prefix_zh: "http_fetch 审批：",
        };
        let allow_handles = crate::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
            cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
        };
        match crate::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(|w| w.approval_sink()),
            cli_ctx.map(|c| crate::tool_approval::CliApprovalInput {
                auto_approve_all_sensitive: c.auto_approve_all_non_whitelist_run_command,
            }),
            &spec,
            "tool_registry::http_fetch approval parallel prefetch",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                failures.insert(key, msg);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                failures.insert(key, "错误：审批通道不可用，请重试。".to_string());
            }
        }
    }
    failures
}

async fn execute_http_fetch_impl(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, method) = match tools::http_fetch::parse_http_fetch_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let key = tools::http_fetch::storage_key(&url);
    let approval_args = tools::http_fetch::approval_args_display(method, &url);
    let allowed_by_cfg =
        tools::http_fetch::url_matches_allowed_prefixes(&url, &cfg.http_fetch_allowed_prefixes);
    let allowed_by_list = match (web_ctx, cli_ctx) {
        (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&key),
        (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&key),
        (None, None) => false,
    };
    if !(allowed_by_cfg || allowed_by_list) {
        if web_ctx.is_none() && cli_ctx.is_none() {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpRead,
            sse_command: "http_fetch".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(key.clone()),
            cli_title: "http_fetch 审批",
            cli_detail: format!(
                "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                approval_args
            ),
            web_timeline_prefix_zh: "http_fetch 审批：",
        };
        let allow_handles = crate::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
            cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
        };
        match crate::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(|w| w.approval_sink()),
            cli_ctx.map(|c| crate::tool_approval::CliApprovalInput {
                auto_approve_all_sensitive: c.auto_approve_all_non_whitelist_run_command,
            }),
            &spec,
            "tool_registry::http_fetch approval",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                return (msg, None);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                return ("错误：审批通道不可用，请重试。".to_string(), None);
            }
        }
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "http_fetch",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;
    let name_in = name.to_string();
    let args_owned = args.to_string();
    let outer_wall = http_fetch_outer_wall_secs(cfg);
    let handle = tokio::task::spawn_blocking(move || {
        let (u, m) = match tools::http_fetch::parse_http_fetch_args(&args_owned) {
            Ok(x) => x,
            Err(e) => return format!("错误：{}", e),
        };
        tools::http_fetch::fetch_with_method(&u, m, timeout_secs, max_body)
    });
    let s = match tokio::time::timeout(Duration::from_secs(outer_wall), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_fetch 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("http_fetch 执行异常：{:?}", e)
        }
        Err(_) => format!("http_fetch 超时（{} 秒）", outer_wall),
    };
    (s, None)
}

async fn execute_http_request_impl(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, method, json_body) = match tools::http_fetch::parse_http_request_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let has_body = json_body.is_some();
    let key = tools::http_fetch::request_storage_key(method, &url);
    let approval_args = tools::http_fetch::approval_args_display_request(method, &url, has_body);
    let allowed_by_cfg =
        tools::http_fetch::url_matches_allowed_prefixes(&url, &cfg.http_fetch_allowed_prefixes);
    let allowed_by_list = match (web_ctx, cli_ctx) {
        (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&key),
        (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&key),
        (None, None) => false,
    };
    if !(allowed_by_cfg || allowed_by_list) {
        if web_ctx.is_none() && cli_ctx.is_none() {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpWrite,
            sse_command: "http_request".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(key.clone()),
            cli_title: "http_request 审批",
            cli_detail: format!(
                "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                approval_args
            ),
            web_timeline_prefix_zh: "http_request 审批：",
        };
        let allow_handles = crate::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
            cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
        };
        match crate::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(|w| w.approval_sink()),
            cli_ctx.map(|c| crate::tool_approval::CliApprovalInput {
                auto_approve_all_sensitive: c.auto_approve_all_non_whitelist_run_command,
            }),
            &spec,
            "tool_registry::http_request approval",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                return (msg, None);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                return ("错误：审批通道不可用，请重试。".to_string(), None);
            }
        }
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "http_request",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;
    let name_in = name.to_string();
    let args_owned = args.to_string();
    let outer_wall = http_request_outer_wall_secs(cfg);
    let handle = tokio::task::spawn_blocking(move || {
        let (u, m, b) = match tools::http_fetch::parse_http_request_args(&args_owned) {
            Ok(x) => x,
            Err(e) => return format!("错误：{}", e),
        };
        tools::http_fetch::request_with_json_body(&u, m, b.as_ref(), timeout_secs, max_body)
    });
    let s = match tokio::time::timeout(Duration::from_secs(outer_wall), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_request 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("http_request 执行异常：{:?}", e)
        }
        Err(_) => format!("http_request 超时（{} 秒）", outer_wall),
    };
    (s, None)
}

async fn execute_run_executable_web(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if !workspace_is_set {
        return (web_tool_err_workspace_not_set("运行可执行程序"), None);
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "run_executable",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let cmd_timeout = cfg.command_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(target: "crabmate", "可执行程序运行超时 tool={}", name);
            format!("可执行程序运行超时（{} 秒）", cmd_timeout)
        }
    };
    (s, None)
}

async fn execute_get_weather_web(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "get_weather",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let weather_timeout = cfg.weather_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(weather_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(target: "crabmate", "天气请求超时 tool={}", name);
            format!("天气请求超时（{} 秒）", weather_timeout)
        }
    };
    (s, None)
}

async fn execute_web_search_web(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        cfg,
        effective_working_dir,
        workspace_is_set,
        "web_search",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let search_timeout = cfg.web_search_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(search_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(target: "crabmate", "联网搜索超时 tool={}", name);
            format!("联网搜索超时（{} 秒）", search_timeout)
        }
    };
    (s, None)
}
