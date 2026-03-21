//! 工具分发注册表：按工具名解析执行策略（workflow / 阻塞+超时 / 同步），Web 与 TUI 共用实现。
//!
//! 新增「需特殊运行时」的工具：在 `HANDLER_MAP` 初始化与 `all_dispatch_metadata()` 中各增一项，并在 `dispatch_tool` 的 `match hid` 中补分支。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};
use tracing::{error, warn};

use crate::config::AgentConfig;
use crate::per_coord::PerCoordinator;
use crate::tools;
use crate::types::{CommandApprovalDecision, ToolCall};
use crate::workflow;
use crate::workflow_reflection_controller;

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
    Web { workspace_changed: &'a mut bool },
    Tui { ctx: &'a TuiToolRuntime },
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
    cfg: &AgentConfig,
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
            ToolRuntime::Web { workspace_changed } => {
                execute_run_command_web(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
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
                warn!(tool = %name, "RunExecutable on TUI without remap; using sync run_tool");
                let ctx =
                    tools::tool_context_for(cfg, &cfg.allowed_commands, effective_working_dir);
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
            ToolRuntime::Web { .. } => execute_http_fetch_web(cfg, name, args).await,
            ToolRuntime::Tui { ctx } => execute_http_fetch_tui(cfg, ctx, name, args).await,
        },
        HandlerId::SyncDefault => {
            let ctx = tools::tool_context_for(cfg, &cfg.allowed_commands, effective_working_dir);
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
    cfg: &AgentConfig,
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
                ToolRuntime::Web { .. } => workflow::WorkflowApprovalMode::NoApproval,
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
                cfg,
                effective_working_dir,
                workspace_is_set,
                approval_mode,
                cfg.command_max_output_len,
            )
            .await;
            if let ToolRuntime::Web { workspace_changed } = runtime {
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
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    workspace_changed: &mut bool,
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
    let name_in = name.to_string();
    let cmd_timeout = cfg.command_timeout_secs;
    let cmd_max_len = cfg.command_max_output_len;
    let weather_secs = cfg.weather_timeout_secs;
    let ws_timeout = cfg.web_search_timeout_secs;
    let ws_provider = cfg.web_search_provider;
    let ws_max = cfg.web_search_max_results;
    let ws_key = cfg.web_search_api_key.clone();
    let hf_pfx = cfg.http_fetch_allowed_prefixes.clone();
    let hf_to = cfg.http_fetch_timeout_secs;
    let hf_mb = cfg.http_fetch_max_response_bytes;
    let allowed = cfg.allowed_commands.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let args_cloned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::ToolContext {
            command_max_output_len: cmd_max_len,
            weather_timeout_secs: weather_secs,
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
        tools::run_tool(&name_in, &args_cloned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(tool = %name, error = ?e, "工具执行异常");
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(tool = %name, "命令执行超时");
            format!("命令执行超时（{} 秒）", cmd_timeout)
        }
    };
    if tools::is_compile_command_success(args, &s) {
        *workspace_changed = true;
    }
    (s, None)
}

async fn execute_run_command_tui(
    cfg: &AgentConfig,
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
            let ctx = tools::tool_context_for(cfg, &effective_allowed, effective_working_dir);
            return (tools::run_tool(name, args, &ctx), None);
        }
        let decision = {
            let _guard = ctx.approval_request_guard.lock().await;
            if let Some(tx) = ctx.out_tx.as_ref() {
                let line = crate::sse_protocol::encode_message(
                    crate::sse_protocol::SsePayload::CommandApproval {
                        command_approval_request: crate::sse_protocol::CommandApprovalBody {
                            command: cmd.clone(),
                            args: arg_preview.clone(),
                            allowlist_key: None,
                        },
                    },
                );
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
                let ctx = tools::tool_context_for(cfg, &effective_allowed, effective_working_dir);
                (tools::run_tool(name, args, &ctx), None)
            }
            CommandApprovalDecision::AllowAlways => {
                ctx.persistent_allowlist_shared
                    .lock()
                    .await
                    .insert(cmd.clone());
                effective_allowed.push(cmd.clone());
                let ctx = tools::tool_context_for(cfg, &effective_allowed, effective_working_dir);
                (tools::run_tool(name, args, &ctx), None)
            }
        }
    } else {
        let ctx = tools::tool_context_for(cfg, &effective_allowed, effective_working_dir);
        (tools::run_tool(name, args, &ctx), None)
    }
}

async fn execute_http_fetch_web(
    cfg: &AgentConfig,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, _method) = match tools::http_fetch::parse_http_fetch_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let url_str = url.as_str().to_string();
    if !tools::http_fetch::url_matches_allowed_prefixes(&url_str, &cfg.http_fetch_allowed_prefixes)
    {
        return (
            "错误：Web 模式下 http_fetch 仅允许 URL 以配置的 http_fetch_allowed_prefixes 中某一前缀开头（参见 README）。TUI 下可对未匹配 URL 使用人工审批（拒绝/本次同意/永久同意）。"
                .to_string(),
            None,
        );
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
            error!(tool = %name_in, error = ?e, "http_fetch 任务异常");
            format!("http_fetch 执行异常：{:?}", e)
        }
        Err(_) => format!("http_fetch 超时（{} 秒）", cmd_timeout),
    };
    (s, None)
}

async fn execute_http_fetch_tui(
    cfg: &AgentConfig,
    ctx: &TuiToolRuntime,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let (url, method) = match tools::http_fetch::parse_http_fetch_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let url_str = url.as_str().to_string();
    let key = tools::http_fetch::storage_key(&url);
    let approval_args = tools::http_fetch::approval_args_display(method, &url);
    let timeout_secs = cfg.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch_max_response_bytes;

    let allowed_by_cfg =
        tools::http_fetch::url_matches_allowed_prefixes(&url_str, &cfg.http_fetch_allowed_prefixes);
    let allowed_by_list = ctx.persistent_allowlist_shared.lock().await.contains(&key);

    if !(allowed_by_cfg || allowed_by_list) {
        let decision = {
            let _guard = ctx.approval_request_guard.lock().await;
            if let Some(tx) = ctx.out_tx.as_ref() {
                let line = crate::sse_protocol::encode_message(
                    crate::sse_protocol::SsePayload::CommandApproval {
                        command_approval_request: crate::sse_protocol::CommandApprovalBody {
                            command: "http_fetch".to_string(),
                            args: approval_args.clone(),
                            allowlist_key: Some(key.clone()),
                        },
                    },
                );
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
            error!(tool = %name, error = ?e, "http_fetch 任务异常");
            format!("http_fetch 执行异常：{:?}", e)
        }
        Err(_) => format!("http_fetch 超时（{} 秒）", cmd_timeout),
    };
    (s, None)
}

async fn execute_run_executable_web(
    cfg: &AgentConfig,
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
    let cmd_max_len = cfg.command_max_output_len;
    let weather_secs = cfg.weather_timeout_secs;
    let ws_timeout = cfg.web_search_timeout_secs;
    let ws_provider = cfg.web_search_provider;
    let ws_max = cfg.web_search_max_results;
    let ws_key = cfg.web_search_api_key.clone();
    let hf_pfx = cfg.http_fetch_allowed_prefixes.clone();
    let hf_to = cfg.http_fetch_timeout_secs;
    let hf_mb = cfg.http_fetch_max_response_bytes;
    let allowed = cfg.allowed_commands.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::ToolContext {
            command_max_output_len: cmd_max_len,
            weather_timeout_secs: weather_secs,
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
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(tool = %name, error = ?e, "工具执行异常");
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(tool = %name, "可执行程序运行超时");
            format!("可执行程序运行超时（{} 秒）", cmd_timeout)
        }
    };
    (s, None)
}

async fn execute_get_weather_web(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let name_in = name.to_string();
    let cmd_max_len = cfg.command_max_output_len;
    let weather_timeout = cfg.weather_timeout_secs;
    let ws_timeout = cfg.web_search_timeout_secs;
    let ws_provider = cfg.web_search_provider;
    let ws_max = cfg.web_search_max_results;
    let ws_key = cfg.web_search_api_key.clone();
    let hf_pfx = cfg.http_fetch_allowed_prefixes.clone();
    let hf_to = cfg.http_fetch_timeout_secs;
    let hf_mb = cfg.http_fetch_max_response_bytes;
    let allowed = cfg.allowed_commands.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::ToolContext {
            command_max_output_len: cmd_max_len,
            weather_timeout_secs: weather_timeout,
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
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(weather_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(tool = %name, error = ?e, "工具执行异常");
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(tool = %name, "天气请求超时");
            format!("天气请求超时（{} 秒）", weather_timeout)
        }
    };
    (s, None)
}

async fn execute_web_search_web(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let name_in = name.to_string();
    let cmd_max_len = cfg.command_max_output_len;
    let weather_timeout = cfg.weather_timeout_secs;
    let search_timeout = cfg.web_search_timeout_secs;
    let ws_provider = cfg.web_search_provider;
    let ws_max = cfg.web_search_max_results;
    let ws_key = cfg.web_search_api_key.clone();
    let hf_pfx = cfg.http_fetch_allowed_prefixes.clone();
    let hf_to = cfg.http_fetch_timeout_secs;
    let hf_mb = cfg.http_fetch_max_response_bytes;
    let allowed = cfg.allowed_commands.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::ToolContext {
            command_max_output_len: cmd_max_len,
            weather_timeout_secs: weather_timeout,
            allowed_commands: &allowed,
            working_dir: &work_dir,
            web_search_timeout_secs: search_timeout,
            web_search_provider: ws_provider,
            web_search_api_key: ws_key.as_str(),
            web_search_max_results: ws_max,
            http_fetch_allowed_prefixes: hf_pfx.as_slice(),
            http_fetch_timeout_secs: hf_to,
            http_fetch_max_response_bytes: hf_mb,
        };
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(search_timeout), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(tool = %name, error = ?e, "工具执行异常");
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(tool = %name, "联网搜索超时");
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
