//! 工具分发注册表：按工具名解析执行策略（workflow / 阻塞+超时 / 同步），Web 与 TUI 共用实现。
//!
//! **`spawn_blocking` 与配置**：进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]（仅增引用计数），闭包内通过 [`tools::tool_context_for`] 借用同一份配置与白名单，避免每次工具调用深度克隆 `allowed_commands`、`http_fetch_allowed_prefixes`、`web_search_api_key` 等大分配。
//!
//! 新增「需特殊运行时」的工具：在 `HANDLER_MAP` 初始化与 `all_dispatch_metadata()` 中各增一项，并在 `dispatch_tool` 的 `match hid` 中补分支。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use log::{error, warn};
use tokio::sync::{Mutex, mpsc};

use crate::agent::per_coord::PerCoordinator;
use crate::agent::workflow;
use crate::agent::workflow_reflection_controller;
use crate::config::AgentConfig;
use crate::tools;
use crate::types::{CommandApprovalDecision, ToolCall};

// --- 元数据（文档 / 将来 OpenAPI 生成）---

/// 工具在运行时的执行类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionClass {
    Workflow,
    CommandSpawnTimeout,
    ExecutableSpawnTimeout,
    WeatherSpawnTimeout,
    WebSearchSpawnTimeout,
    HttpFetchSpawnTimeout,
    BlockingSync,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ToolDispatchMeta {
    pub name: &'static str,
    pub requires_workspace: bool,
    pub class: ToolExecutionClass,
}

/// 注册表中显式声明的工具；其余名称运行时走 `SyncDefault`（同步 `run_tool`）。
pub fn all_dispatch_metadata() -> &'static [ToolDispatchMeta] {
    &[
        ToolDispatchMeta {
            name: "workflow_execute",
            requires_workspace: false,
            class: ToolExecutionClass::Workflow,
        },
        ToolDispatchMeta {
            name: "run_command",
            requires_workspace: true,
            class: ToolExecutionClass::CommandSpawnTimeout,
        },
        ToolDispatchMeta {
            name: "run_executable",
            requires_workspace: true,
            class: ToolExecutionClass::ExecutableSpawnTimeout,
        },
        ToolDispatchMeta {
            name: "get_weather",
            requires_workspace: false,
            class: ToolExecutionClass::WeatherSpawnTimeout,
        },
        ToolDispatchMeta {
            name: "web_search",
            requires_workspace: false,
            class: ToolExecutionClass::WebSearchSpawnTimeout,
        },
        ToolDispatchMeta {
            name: "http_fetch",
            requires_workspace: false,
            class: ToolExecutionClass::HttpFetchSpawnTimeout,
        },
        ToolDispatchMeta {
            name: "http_request",
            requires_workspace: false,
            class: ToolExecutionClass::HttpFetchSpawnTimeout,
        },
    ]
}

/// 若在 `all_dispatch_metadata` 中登记则返回其元数据，否则 `None`（运行时走同步 `run_tool`）。
pub fn try_dispatch_meta(name: &str) -> Option<&'static ToolDispatchMeta> {
    meta_by_name(name)
}

/// 合并「注册表元数据 + 默认同步」的执行类别，便于文档或将来生成 OpenAPI。
pub fn execution_class_for_tool(name: &str) -> ToolExecutionClass {
    try_dispatch_meta(name)
        .map(|m| m.class)
        .unwrap_or(ToolExecutionClass::BlockingSync)
}

fn meta_by_name(name: &str) -> Option<&'static ToolDispatchMeta> {
    all_dispatch_metadata().iter().find(|m| m.name == name)
}

// --- 运行时上下文 ---

pub enum ToolRuntime<'a> {
    Web {
        workspace_changed: &'a mut bool,
        /// 仅 Web 流式会话在启用审批时提供；普通 `/chat` 或旧客户端为 `None`。
        ctx: Option<&'a WebToolRuntime>,
    },
    Tui {
        ctx: &'a TuiToolRuntime,
    },
}

pub struct WebToolRuntime {
    pub out_tx: mpsc::Sender<String>,
    pub approval_rx_shared: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<Mutex<()>>,
    pub persistent_allowlist_shared: Arc<Mutex<HashSet<String>>>,
}

pub struct TuiToolRuntime {
    pub out_tx: Option<mpsc::Sender<String>>,
    pub approval_rx_shared: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<Mutex<()>>,
    pub persistent_allowlist_shared: Arc<Mutex<HashSet<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HandlerId {
    Workflow,
    RunCommand,
    RunExecutable,
    GetWeather,
    WebSearch,
    HttpFetch,
    HttpRequest,
    SyncDefault,
}

static HANDLER_MAP: OnceLock<HashMap<&'static str, HandlerId>> = OnceLock::new();

fn handler_id_for(name: &str) -> HandlerId {
    HANDLER_MAP
        .get_or_init(|| {
            let mut m = HashMap::new();
            m.insert("workflow_execute", HandlerId::Workflow);
            m.insert("run_command", HandlerId::RunCommand);
            m.insert("run_executable", HandlerId::RunExecutable);
            m.insert("get_weather", HandlerId::GetWeather);
            m.insert("web_search", HandlerId::WebSearch);
            m.insert("http_fetch", HandlerId::HttpFetch);
            m.insert("http_request", HandlerId::HttpRequest);
            m
        })
        .get(name)
        .copied()
        .unwrap_or(HandlerId::SyncDefault)
}

/// Web/TUI 统一入口：`(tool_result_text, workflow 反思注入)`。
#[allow(clippy::too_many_arguments)]
pub async fn dispatch_tool(
    runtime: ToolRuntime<'_>,
    per_coord: &mut PerCoordinator,
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
    tc: &ToolCall,
) -> (String, Option<serde_json::Value>) {
    let mut hid = handler_id_for(name);
    if matches!(
        hid,
        HandlerId::RunExecutable | HandlerId::GetWeather | HandlerId::WebSearch
    ) && matches!(runtime, ToolRuntime::Tui { .. })
    {
        hid = HandlerId::SyncDefault;
    }

    match hid {
        HandlerId::Workflow => {
            execute_workflow(
                runtime,
                per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                args,
            )
            .await
        }
        HandlerId::RunCommand => match runtime {
            ToolRuntime::Web {
                workspace_changed,
                ctx,
            } => {
                execute_run_command_web(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
                    ctx,
                    name,
                    args,
                )
                .await
            }
            ToolRuntime::Tui { ctx } => {
                execute_run_command_tui(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    ctx,
                    name,
                    args,
                )
                .await
            }
        },
        HandlerId::RunExecutable => match runtime {
            ToolRuntime::Web { .. } => {
                execute_run_executable_web(cfg, effective_working_dir, workspace_is_set, name, args)
                    .await
            }
            ToolRuntime::Tui { .. } => {
                // TUI 入口通常将 RunExecutable remap 为 SyncDefault；若未 remap，退回通用 run_tool 以免 panic。
                warn!(
                    target: "crabmate",
                    "RunExecutable on TUI without remap; using sync run_tool tool={}",
                    name
                );
                let ctx = tools::tool_context_for(
                    cfg.as_ref(),
                    &cfg.allowed_commands,
                    effective_working_dir,
                );
                (
                    tools::run_tool(&tc.function.name, &tc.function.arguments, &ctx),
                    None,
                )
            }
        },
        HandlerId::GetWeather => {
            execute_get_weather_web(cfg, effective_working_dir, name, args).await
        }
        HandlerId::WebSearch => {
            execute_web_search_web(cfg, effective_working_dir, name, args).await
        }
        HandlerId::HttpFetch => match runtime {
            ToolRuntime::Web { ctx, .. } => execute_http_fetch_web(cfg, ctx, name, args).await,
            ToolRuntime::Tui { ctx } => execute_http_fetch_tui(cfg, ctx, name, args).await,
        },
        HandlerId::HttpRequest => execute_http_request(cfg, name, args).await,
        HandlerId::SyncDefault => {
            let ctx =
                tools::tool_context_for(cfg.as_ref(), &cfg.allowed_commands, effective_working_dir);
            (
                tools::run_tool(&tc.function.name, &tc.function.arguments, &ctx),
                None,
            )
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
            let approval_mode = match &runtime {
                ToolRuntime::Web { ctx, .. } => {
                    if let Some(web_ctx) = ctx {
                        workflow::WorkflowApprovalMode::Tui {
                            out_tx: web_ctx.out_tx.clone(),
                            approval_rx: web_ctx.approval_rx_shared.clone(),
                            approval_request_guard: web_ctx.approval_request_guard.clone(),
                            persistent_allowlist: web_ctx.persistent_allowlist_shared.clone(),
                        }
                    } else {
                        workflow::WorkflowApprovalMode::NoApproval
                    }
                }
                ToolRuntime::Tui { ctx } => {
                    if let Some(tx) = ctx.out_tx.as_ref() {
                        workflow::WorkflowApprovalMode::Tui {
                            out_tx: tx.clone(),
                            approval_rx: ctx.approval_rx_shared.clone(),
                            approval_request_guard: ctx.approval_request_guard.clone(),
                            persistent_allowlist: ctx.persistent_allowlist_shared.clone(),
                        }
                    } else {
                        workflow::WorkflowApprovalMode::NoApproval
                    }
                }
            };
            let (wf_out, wf_ws_changed) = workflow::run_workflow_execute_tool(
                &prep.patched_args,
                cfg.as_ref(),
                effective_working_dir,
                workspace_is_set,
                approval_mode,
                cfg.command_max_output_len,
            )
            .await;
            if let ToolRuntime::Web {
                workspace_changed, ..
            } = runtime
            {
                *workspace_changed |= wf_ws_changed;
            }
            wf_out
        }
    } else {
        prep.skipped_result.clone()
    };

    (result, reflection_inject)
}

async fn execute_run_command_web(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    workspace_changed: &mut bool,
    web_ctx: Option<&WebToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if !workspace_is_set {
        return (
            "错误：未设置工作区，禁止执行命令。请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。"
                .to_string(),
            None,
        );
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
    let mut effective_allowed = cfg.allowed_commands.clone();
    if !cmd.is_empty()
        && !effective_allowed
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd))
    {
        let already_allowed = if let Some(ctx) = web_ctx {
            ctx.persistent_allowlist_shared.lock().await.contains(&cmd)
        } else {
            false
        };
        if already_allowed {
            effective_allowed.push(cmd.clone());
        } else if let Some(ctx) = web_ctx {
            let decision = {
                let _guard = ctx.approval_request_guard.lock().await;
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: cmd.clone(),
                        args: arg_preview.clone(),
                        allowlist_key: None,
                    },
                });
                if ctx.out_tx.send(line).await.is_err() {
                    return ("错误：审批通道不可用，请重试。".to_string(), None);
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            match decision {
                CommandApprovalDecision::Deny => {
                    let cmd_show = if arg_preview.is_empty() {
                        cmd
                    } else {
                        format!("{} {}", cmd, arg_preview)
                    };
                    return (format!("用户拒绝执行命令：{}", cmd_show.trim()), None);
                }
                CommandApprovalDecision::AllowOnce => {
                    effective_allowed.push(cmd.clone());
                }
                CommandApprovalDecision::AllowAlways => {
                    ctx.persistent_allowlist_shared
                        .lock()
                        .await
                        .insert(cmd.clone());
                    effective_allowed.push(cmd.clone());
                }
            }
        }
    }

    let name_in = name.to_string();
    let cmd_timeout = cfg.command_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_cloned = args.to_string();
    let effective_allowed_for_run = effective_allowed.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            effective_allowed_for_run.as_slice(),
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

async fn execute_run_command_tui(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    ctx: &TuiToolRuntime,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    if !workspace_is_set {
        return ("错误：未设置工作区，禁止执行命令。".to_string(), None);
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
    let mut effective_allowed = cfg.allowed_commands.clone();
    if !cmd.is_empty()
        && !effective_allowed
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd))
    {
        let already_allowed = ctx.persistent_allowlist_shared.lock().await.contains(&cmd);
        if already_allowed {
            effective_allowed.push(cmd.clone());
            let ctx =
                tools::tool_context_for(cfg.as_ref(), &effective_allowed, effective_working_dir);
            return (tools::run_tool(name, args, &ctx), None);
        }
        let decision = {
            let _guard = ctx.approval_request_guard.lock().await;
            if let Some(tx) = ctx.out_tx.as_ref() {
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: cmd.clone(),
                        args: arg_preview.clone(),
                        allowlist_key: None,
                    },
                });
                let _ = tx.send(line).await;
            }
            let mut rx_guard = ctx.approval_rx_shared.lock().await;
            rx_guard
                .recv()
                .await
                .unwrap_or(CommandApprovalDecision::Deny)
        };
        match decision {
            CommandApprovalDecision::Deny => {
                let cmd_show = if arg_preview.is_empty() {
                    cmd
                } else {
                    format!("{} {}", cmd, arg_preview)
                };
                (format!("用户拒绝执行命令：{}", cmd_show.trim()), None)
            }
            CommandApprovalDecision::AllowOnce => {
                effective_allowed.push(cmd.clone());
                let ctx = tools::tool_context_for(
                    cfg.as_ref(),
                    &effective_allowed,
                    effective_working_dir,
                );
                (tools::run_tool(name, args, &ctx), None)
            }
            CommandApprovalDecision::AllowAlways => {
                ctx.persistent_allowlist_shared
                    .lock()
                    .await
                    .insert(cmd.clone());
                effective_allowed.push(cmd.clone());
                let ctx = tools::tool_context_for(
                    cfg.as_ref(),
                    &effective_allowed,
                    effective_working_dir,
                );
                (tools::run_tool(name, args, &ctx), None)
            }
        }
    } else {
        let ctx = tools::tool_context_for(cfg.as_ref(), &effective_allowed, effective_working_dir);
        (tools::run_tool(name, args, &ctx), None)
    }
}

async fn execute_http_fetch_web(
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
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
    let allowed_by_list = if let Some(ctx) = web_ctx {
        ctx.persistent_allowlist_shared.lock().await.contains(&key)
    } else {
        false
    };
    if !(allowed_by_cfg || allowed_by_list) {
        if let Some(ctx) = web_ctx {
            let decision = {
                let _guard = ctx.approval_request_guard.lock().await;
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: "http_fetch".to_string(),
                        args: approval_args.clone(),
                        allowlist_key: Some(key.clone()),
                    },
                });
                if ctx.out_tx.send(line).await.is_err() {
                    return ("错误：审批通道不可用，请重试。".to_string(), None);
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            match decision {
                CommandApprovalDecision::Deny => {
                    return (format!("用户拒绝 http_fetch：{}", approval_args), None);
                }
                CommandApprovalDecision::AllowOnce => {}
                CommandApprovalDecision::AllowAlways => {
                    ctx.persistent_allowlist_shared
                        .lock()
                        .await
                        .insert(key.clone());
                }
            }
        } else {
            return (
                "错误：Web 模式下 http_fetch 仅允许匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。若需人工审批，请升级前端并在 /chat/stream 传 approval_session_id。"
                    .to_string(),
                None,
            );
        }
    }
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;
    let name_in = name.to_string();
    let args_owned = args.to_string();
    let cmd_timeout = cfg.command_timeout_secs.max(timeout_secs);
    let handle = tokio::task::spawn_blocking(move || {
        let (u, m) = match tools::http_fetch::parse_http_fetch_args(&args_owned) {
            Ok(x) => x,
            Err(e) => return format!("错误：{}", e),
        };
        tools::http_fetch::fetch_with_method(&u, m, timeout_secs, max_body)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
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
        Err(_) => format!("http_fetch 超时（{} 秒）", cmd_timeout),
    };
    (s, None)
}

async fn execute_http_fetch_tui(
    cfg: &Arc<AgentConfig>,
    ctx: &TuiToolRuntime,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, method) = match tools::http_fetch::parse_http_fetch_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let key = tools::http_fetch::storage_key(&url);
    let approval_args = tools::http_fetch::approval_args_display(method, &url);
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;

    let allowed_by_cfg =
        tools::http_fetch::url_matches_allowed_prefixes(&url, &cfg.http_fetch_allowed_prefixes);
    let allowed_by_list = ctx.persistent_allowlist_shared.lock().await.contains(&key);

    if !(allowed_by_cfg || allowed_by_list) {
        let decision = {
            let _guard = ctx.approval_request_guard.lock().await;
            if let Some(tx) = ctx.out_tx.as_ref() {
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: "http_fetch".to_string(),
                        args: approval_args.clone(),
                        allowlist_key: Some(key.clone()),
                    },
                });
                let _ = tx.send(line).await;
            }
            let mut rx_guard = ctx.approval_rx_shared.lock().await;
            rx_guard
                .recv()
                .await
                .unwrap_or(CommandApprovalDecision::Deny)
        };
        match decision {
            CommandApprovalDecision::Deny => {
                return (format!("用户拒绝 http_fetch：{}", approval_args), None);
            }
            CommandApprovalDecision::AllowOnce => {}
            CommandApprovalDecision::AllowAlways => {
                ctx.persistent_allowlist_shared
                    .lock()
                    .await
                    .insert(key.clone());
            }
        }
    }

    let args_owned = args.to_string();
    let cmd_timeout = cfg.command_timeout_secs.max(timeout_secs);
    let handle = tokio::task::spawn_blocking(move || {
        let (u, m) = match tools::http_fetch::parse_http_fetch_args(&args_owned) {
            Ok(x) => x,
            Err(e) => return format!("错误：{}", e),
        };
        tools::http_fetch::fetch_with_method(&u, m, timeout_secs, max_body)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_fetch 任务异常 tool={} error={:?}",
                name,
                e
            );
            format!("http_fetch 执行异常：{:?}", e)
        }
        Err(_) => format!("http_fetch 超时（{} 秒）", cmd_timeout),
    };
    (s, None)
}

async fn execute_http_request(
    cfg: &Arc<AgentConfig>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, _method, _json_body) = match tools::http_fetch::parse_http_request_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    if !tools::http_fetch::url_matches_allowed_prefixes(&url, &cfg.http_fetch_allowed_prefixes) {
        return (
            "错误：http_request 仅允许匹配配置的 http_fetch_allowed_prefixes（同源 + 路径前缀边界）。"
                .to_string(),
            None,
        );
    }
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;
    let args_owned = args.to_string();
    let cmd_timeout = cfg.command_timeout_secs.max(timeout_secs);
    let handle = tokio::task::spawn_blocking(move || {
        let (u, m, b) = match tools::http_fetch::parse_http_request_args(&args_owned) {
            Ok(x) => x,
            Err(e) => return format!("错误：{}", e),
        };
        tools::http_fetch::request_with_json_body(&u, m, b.as_ref(), timeout_secs, max_body)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_request 任务异常 tool={} error={:?}",
                name,
                e
            );
            format!("http_request 执行异常：{:?}", e)
        }
        Err(_) => format!("http_request 超时（{} 秒）", cmd_timeout),
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
        return (
            "错误：未设置工作区，禁止运行可执行程序。请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。"
                .to_string(),
            None,
        );
    }
    let name_in = name.to_string();
    let cmd_timeout = cfg.command_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_slice(),
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
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let name_in = name.to_string();
    let weather_timeout = cfg.weather_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_slice(),
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
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let name_in = name.to_string();
    let search_timeout = cfg.web_search_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.allowed_commands.as_slice(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_map_resolves_known_tools() {
        assert_eq!(handler_id_for("workflow_execute"), HandlerId::Workflow);
        assert_eq!(handler_id_for("run_command"), HandlerId::RunCommand);
        assert_eq!(handler_id_for("web_search"), HandlerId::WebSearch);
        assert_eq!(handler_id_for("http_request"), HandlerId::HttpRequest);
        assert_eq!(handler_id_for("unknown_xyz"), HandlerId::SyncDefault);
    }

    #[test]
    fn try_dispatch_meta_unknown_is_none() {
        assert!(try_dispatch_meta("calc").is_none());
        assert_eq!(
            try_dispatch_meta("workflow_execute").map(|m| m.name),
            Some("workflow_execute")
        );
    }

    #[test]
    fn meta_fields_and_default_class() {
        let wf = try_dispatch_meta("workflow_execute").unwrap();
        assert!(!wf.requires_workspace);
        assert_eq!(wf.class, ToolExecutionClass::Workflow);
        let rc = try_dispatch_meta("run_command").unwrap();
        assert!(rc.requires_workspace);
        assert_eq!(rc.class, ToolExecutionClass::CommandSpawnTimeout);
        assert_eq!(
            execution_class_for_tool("calc"),
            ToolExecutionClass::BlockingSync
        );
    }
}
