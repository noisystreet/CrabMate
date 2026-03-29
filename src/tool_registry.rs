//! 工具分发注册表：按工具名解析执行策略（workflow / 阻塞+超时 / 同步），Web 与 TUI 共用实现。
//!
//! **`spawn_blocking` 与配置**：进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]（仅增引用计数），闭包内通过 [`tools::tool_context_for`] 借用同一份配置与白名单；`allowed_commands` 在 [`AgentConfig`] 内为 [`std::sync::Arc`] 共享切片，避免每轮工具调用整表克隆。纯 CPU、无阻塞 IO 的少数工具可走 [`sync_default_runs_inline`] 在当前 async 任务上直接执行。
//!
//! 新增「需特殊运行时」的工具：在 `HANDLER_MAP` 初始化与 `all_dispatch_metadata()` 中各增一项，并在 `dispatch_tool` 的 `match hid` 中补分支。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::time::Duration;

use log::error;
use tokio::sync::{Mutex, mpsc};

use crate::agent::per_coord::PerCoordinator;
use crate::agent::workflow;
use crate::agent::workflow_reflection_controller;
use crate::config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::tools;
use crate::types::{CommandApprovalDecision, ToolCall};

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

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
/// 写操作工具（create/modify/delete/move/copy/format/apply_patch 等）及带审批的工具返回 false。
pub fn is_readonly_tool(name: &str) -> bool {
    if crate::mcp::is_mcp_proxy_tool(name) {
        // 外部 MCP 工具语义未知，禁止与内建只读工具并行同批执行。
        return false;
    }
    static WRITE_TOOLS: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    let writes = WRITE_TOOLS.get_or_init(|| {
        [
            "create_file",
            "modify_file",
            "copy_file",
            "move_file",
            "delete_file",
            "delete_dir",
            "append_file",
            "create_dir",
            "search_replace",
            "chmod_file",
            "apply_patch",
            "format_file",
            "ast_grep_rewrite",
            "structured_patch",
            "git_stage_files",
            "git_commit",
            "git_checkout",
            "git_branch_create",
            "git_branch_delete",
            "git_push",
            "git_merge",
            "git_rebase",
            "git_stash",
            "git_tag",
            "git_reset",
            "git_cherry_pick",
            "git_revert",
            "git_clone",
            "git_remote_set_url",
            "git_apply",
            "git_fetch",
            "cargo_fix",
            "cargo_clean",
            "python_install_editable",
            "npm_install",
            "go_mod_tidy",
            "run_command",
            "run_executable",
            "workflow_execute",
            "http_request",
        ]
        .into_iter()
        .collect()
    });
    !writes.contains(name)
}

/// 即使 [`is_readonly_tool`] 为真，并行 `spawn_blocking` 仍可能争抢 cargo/npm 等构建锁或缓存；勿与同批其它工具并行。
fn parallel_sync_batch_denied(name: &str) -> bool {
    matches!(
        name,
        "rust_compiler_json" | "quality_workspace" | "ci_pipeline_local" | "repo_overview_sweep"
    ) || name.starts_with("cargo_")
        || name.starts_with("npm_")
        || name.starts_with("frontend_")
        || name.starts_with("go_")
        || name.starts_with("ruff_")
        || name.starts_with("pytest")
        || name.starts_with("mypy_")
        || name.starts_with("uv_")
        || name.starts_with("pre_commit")
        || name.starts_with("python_")
        || name.starts_with("typos_")
        || name.starts_with("codespell_")
}

/// 可与其它只读工具同批 **并行** 执行的工具（不含 `http_request`、命令类、MCP）。
///
/// - **`SyncDefault`**：内建只读且非 `parallel_sync_batch_denied`。
/// - **`http_fetch`**：GET/HEAD 只读；审批在并行 `spawn_blocking` 之前**串行**完成（见 `execute_tools`）。
/// - **`get_weather` / `web_search`**：出站只读 HTTP；无工作区副作用，可与 `read_file` 等同批并行。
fn parallel_batch_eligible_tool(name: &str) -> bool {
    if parallel_sync_batch_denied(name) {
        return false;
    }
    matches!(
        handler_id_for(name),
        HandlerId::SyncDefault
            | HandlerId::HttpFetch
            | HandlerId::GetWeather
            | HandlerId::WebSearch
    )
}

/// 单工具是否满足「可与其它同类工具同批并行」的语义（不含「至少 2 个调用」前提）。
///
/// 与 [`tool_calls_allow_parallel_sync_batch`] 中每个 `ToolCall` 的判定一致；供分阶段规划**优化轮**提示词列举可批量并行的内建工具名。
pub fn tool_ok_for_parallel_readonly_batch_piece(name: &str) -> bool {
    !crate::mcp::is_mcp_proxy_tool(name)
        && is_readonly_tool(name)
        && parallel_batch_eligible_tool(name)
}

/// 本批 **至少 2 个** 工具且全部为语义只读、且均为 [`parallel_batch_eligible_tool`] 时，可在单轮内并行执行
///（`SyncDefault` / `http_fetch` / `get_weather` / `web_search`；**不含** `http_request`、命令类、MCP；`http_fetch` 的审批先于并行 IO，见 `agent_turn::per_execute_tools_common`）。
pub fn tool_calls_allow_parallel_sync_batch(tool_calls: &[ToolCall]) -> bool {
    tool_calls.len() > 1
        && tool_calls
            .iter()
            .all(|tc| tool_ok_for_parallel_readonly_batch_piece(tc.function.name.as_str()))
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
    /// 终端 CLI：`run_command` 非白名单时走 stdin 确认（与 Web 审批语义一致）。
    Cli {
        workspace_changed: &'a mut bool,
        ctx: &'a CliToolRuntime,
    },
}

pub struct WebToolRuntime {
    pub out_tx: mpsc::Sender<String>,
    pub approval_rx_shared: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<Mutex<()>>,
    pub persistent_allowlist_shared: Arc<Mutex<HashSet<String>>>,
}

/// CLI 统计：用于 `chat` 退出码（本进程内 `run_command` 调用次数与用户拒绝次数）。
#[derive(Debug, Default, Clone, Copy)]
pub struct CliCommandTurnStats {
    pub run_command_attempts: u32,
    pub run_command_denials: u32,
}

/// CLI REPL / 单次提问：对**不在** `allowed_commands` 的 `run_command` 在终端 stdin 交互确认；**永久允许**写入本结构（进程内）。
#[derive(Clone)]
pub struct CliToolRuntime {
    pub persistent_allowlist_shared: Arc<Mutex<HashSet<String>>>,
    /// `--yes`：非白名单也自动批准（**仅可信环境**；脚本/CI 无人值守）。
    pub auto_approve_all_non_whitelist_run_command: bool,
    /// `--approve-commands` 额外允许的命令名（小写），与配置白名单合并后再决定是否提示。
    pub extra_allowlist_commands: Arc<[String]>,
    pub command_stats: Arc<StdMutex<CliCommandTurnStats>>,
}

impl CliToolRuntime {
    /// REPL / 默认单次问答：交互审批，不自动批准。
    pub fn new_interactive_default() -> Self {
        Self {
            persistent_allowlist_shared: Arc::new(Mutex::new(HashSet::new())),
            auto_approve_all_non_whitelist_run_command: false,
            extra_allowlist_commands: Arc::from([] as [String; 0]),
            command_stats: Arc::new(StdMutex::new(CliCommandTurnStats::default())),
        }
    }

    pub fn reset_command_stats(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            *g = CliCommandTurnStats::default();
        }
    }

    fn record_run_command_attempt(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_attempts = g.run_command_attempts.saturating_add(1);
        }
    }

    fn record_run_command_denial(&self) {
        if let Ok(mut g) = self.command_stats.lock() {
            g.run_command_denials = g.run_command_denials.saturating_add(1);
        }
    }

    /// 本回合（自上次 [`Self::reset_command_stats`]）内每次 `run_command` 均被用户拒绝。
    pub fn all_run_commands_were_denied(&self) -> bool {
        self.command_stats.lock().is_ok_and(|g| {
            g.run_command_attempts > 0 && g.run_command_denials == g.run_command_attempts
        })
    }
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

/// 无子进程、无阻塞网络/磁盘的 `SyncDefault` 工具：跳过 `spawn_blocking`，以免线程池调度开销大于工具本身。
pub(crate) fn sync_default_runs_inline(name: &str) -> bool {
    matches!(name, "get_current_time" | "convert_units")
}

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

/// Web / CLI 统一入口：`(tool_result_text, workflow 反思注入)`。
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
    read_file_turn_cache: Option<std::sync::Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    mcp_session: Option<&Arc<Mutex<crate::mcp::McpClientSession>>>,
) -> (String, Option<serde_json::Value>) {
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
        HandlerId::HttpFetch => match runtime {
            ToolRuntime::Web { ctx, .. } => {
                execute_http_fetch_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    ctx,
                    None,
                    name,
                    args,
                )
                .await
            }
            ToolRuntime::Cli { ctx, .. } => {
                execute_http_fetch_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    None,
                    Some(ctx),
                    name,
                    args,
                )
                .await
            }
        },
        HandlerId::HttpRequest => match runtime {
            ToolRuntime::Web { ctx, .. } => {
                execute_http_request_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    ctx,
                    None,
                    name,
                    args,
                )
                .await
            }
            ToolRuntime::Cli { ctx, .. } => {
                execute_http_request_impl(
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    None,
                    Some(ctx),
                    name,
                    args,
                )
                .await
            }
        },
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
            if sync_default_runs_inline(name) {
                let ctx = tools::tool_context_for_with_read_cache(
                    cfg.as_ref(),
                    cfg.allowed_commands.as_ref(),
                    effective_working_dir,
                    read_file_turn_cache.as_ref().map(|a| a.as_ref()),
                );
                return (tools::run_tool(name, args, &ctx), None);
            }
            let cfg2 = Arc::clone(cfg);
            let tool_name = tc.function.name.clone();
            let tool_args = tc.function.arguments.clone();
            let work_dir = effective_working_dir.to_path_buf();
            let rfc = read_file_turn_cache.clone();
            let result = tokio::task::spawn_blocking(move || {
                let ctx = tools::tool_context_for_with_read_cache(
                    cfg2.as_ref(),
                    cfg2.allowed_commands.as_ref(),
                    work_dir.as_path(),
                    rfc.as_ref().map(|a| a.as_ref()),
                );
                tools::run_tool(&tool_name, &tool_args, &ctx)
            })
            .await
            .unwrap_or_else(|e| format!("工具执行 panic：{}", e));
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
                if !crate::sse::send_string_logged(
                    &ctx.out_tx,
                    line,
                    "tool_registry::run_command approval",
                )
                .await
                {
                    return ("错误：审批通道不可用，请重试。".to_string(), None);
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            let cmd_show = if arg_preview.is_empty() {
                cmd.clone()
            } else {
                format!("{} {}", cmd, arg_preview)
            };
            crate::sse::web_approval::send_timeline_approval_decision(
                &ctx.out_tx,
                "命令审批：",
                Some(cmd_show.trim().to_string()),
                decision,
                "tool_registry::run_command approval timeline",
            )
            .await;
            match decision {
                CommandApprovalDecision::Deny => {
                    return (format!("用户拒绝执行命令：{}", cmd_show.trim()), None);
                }
                CommandApprovalDecision::AllowOnce => {
                    effective_allowed_arc =
                        extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
                }
                CommandApprovalDecision::AllowAlways => {
                    ctx.persistent_allowlist_shared
                        .lock()
                        .await
                        .insert(cmd.clone());
                    effective_allowed_arc =
                        extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
                }
            }
        } else if let Some(ctx) = cli_ctx {
            let cmd_show = if arg_preview.is_empty() {
                cmd.clone()
            } else {
                format!("{} {}", cmd, arg_preview)
            };
            if ctx.auto_approve_all_non_whitelist_run_command
                || ctx
                    .extra_allowlist_commands
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&cmd))
            {
                effective_allowed_arc = extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
            } else {
                let detail = format!("命令不在白名单:\n{}", cmd_show.trim());
                let decision = crate::runtime::cli_approval::prompt_tool_approval_cli(
                    "run_command 审批",
                    &detail,
                )
                .await;
                match decision {
                    CommandApprovalDecision::Deny => {
                        ctx.record_run_command_denial();
                        return (format!("用户拒绝执行命令：{}", cmd_show.trim()), None);
                    }
                    CommandApprovalDecision::AllowOnce => {
                        effective_allowed_arc =
                            extend_allowed_commands_arc(&cfg.allowed_commands, &cmd);
                    }
                    CommandApprovalDecision::AllowAlways => {
                        ctx.persistent_allowlist_shared
                            .lock()
                            .await
                            .insert(cmd.clone());
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
        if let Some(ctx) = web_ctx {
            let decision = {
                let _guard = ctx.approval_request_guard.lock().await;
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: "http_fetch".to_string(),
                        args: approval_args.clone(),
                        allowlist_key: Some(storage_key.clone()),
                    },
                });
                if !crate::sse::send_string_logged(
                    &ctx.out_tx,
                    line,
                    "tool_registry::http_fetch approval parallel prefetch",
                )
                .await
                {
                    failures.insert(key, "错误：审批通道不可用，请重试。".to_string());
                    continue;
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            crate::sse::web_approval::send_timeline_approval_decision(
                &ctx.out_tx,
                "http_fetch 审批：",
                Some(approval_args.clone()),
                decision,
                "tool_registry::http_fetch approval timeline parallel prefetch",
            )
            .await;
            match decision {
                CommandApprovalDecision::Deny => {
                    failures.insert(key, format!("用户拒绝 http_fetch：{}", approval_args));
                }
                CommandApprovalDecision::AllowOnce => {}
                CommandApprovalDecision::AllowAlways => {
                    ctx.persistent_allowlist_shared
                        .lock()
                        .await
                        .insert(storage_key);
                }
            }
        } else if let Some(cli_ctx) = cli_ctx {
            if !cli_ctx.auto_approve_all_non_whitelist_run_command {
                let detail = format!(
                    "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                    approval_args
                );
                let decision = crate::runtime::cli_approval::prompt_tool_approval_cli(
                    "http_fetch 审批",
                    &detail,
                )
                .await;
                match decision {
                    CommandApprovalDecision::Deny => {
                        failures.insert(key, format!("用户拒绝 http_fetch：{}", approval_args));
                    }
                    CommandApprovalDecision::AllowOnce => {}
                    CommandApprovalDecision::AllowAlways => {
                        cli_ctx
                            .persistent_allowlist_shared
                            .lock()
                            .await
                            .insert(storage_key);
                    }
                }
            }
        } else {
            failures.insert(
                key,
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
            );
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
                if !crate::sse::send_string_logged(
                    &ctx.out_tx,
                    line,
                    "tool_registry::http_fetch approval",
                )
                .await
                {
                    return ("错误：审批通道不可用，请重试。".to_string(), None);
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            crate::sse::web_approval::send_timeline_approval_decision(
                &ctx.out_tx,
                "http_fetch 审批：",
                Some(approval_args.clone()),
                decision,
                "tool_registry::http_fetch approval timeline",
            )
            .await;
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
        } else if let Some(cli_ctx) = cli_ctx {
            // `--yes`：与 `run_command` 对齐，非白名单 URL 亦直接放行（仅可信环境）
            if !cli_ctx.auto_approve_all_non_whitelist_run_command {
                let detail = format!(
                    "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                    approval_args
                );
                let decision = crate::runtime::cli_approval::prompt_tool_approval_cli(
                    "http_fetch 审批",
                    &detail,
                )
                .await;
                match decision {
                    CommandApprovalDecision::Deny => {
                        return (format!("用户拒绝 http_fetch：{}", approval_args), None);
                    }
                    CommandApprovalDecision::AllowOnce => {}
                    CommandApprovalDecision::AllowAlways => {
                        cli_ctx
                            .persistent_allowlist_shared
                            .lock()
                            .await
                            .insert(key.clone());
                    }
                }
            }
        } else {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
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
        if let Some(ctx) = web_ctx {
            let decision = {
                let _guard = ctx.approval_request_guard.lock().await;
                let line = crate::sse::encode_message(crate::sse::SsePayload::CommandApproval {
                    command_approval_request: crate::sse::CommandApprovalBody {
                        command: "http_request".to_string(),
                        args: approval_args.clone(),
                        allowlist_key: Some(key.clone()),
                    },
                });
                if !crate::sse::send_string_logged(
                    &ctx.out_tx,
                    line,
                    "tool_registry::http_request approval",
                )
                .await
                {
                    return ("错误：审批通道不可用，请重试。".to_string(), None);
                }
                let mut rx_guard = ctx.approval_rx_shared.lock().await;
                rx_guard
                    .recv()
                    .await
                    .unwrap_or(CommandApprovalDecision::Deny)
            };
            crate::sse::web_approval::send_timeline_approval_decision(
                &ctx.out_tx,
                "http_request 审批：",
                Some(approval_args.clone()),
                decision,
                "tool_registry::http_request approval timeline",
            )
            .await;
            match decision {
                CommandApprovalDecision::Deny => {
                    return (format!("用户拒绝 http_request：{}", approval_args), None);
                }
                CommandApprovalDecision::AllowOnce => {}
                CommandApprovalDecision::AllowAlways => {
                    ctx.persistent_allowlist_shared
                        .lock()
                        .await
                        .insert(key.clone());
                }
            }
        } else if let Some(cli_ctx) = cli_ctx {
            if !cli_ctx.auto_approve_all_non_whitelist_run_command {
                let detail = format!(
                    "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                    approval_args
                );
                let decision = crate::runtime::cli_approval::prompt_tool_approval_cli(
                    "http_request 审批",
                    &detail,
                )
                .await;
                match decision {
                    CommandApprovalDecision::Deny => {
                        return (format!("用户拒绝 http_request：{}", approval_args), None);
                    }
                    CommandApprovalDecision::AllowOnce => {}
                    CommandApprovalDecision::AllowAlways => {
                        cli_ctx
                            .persistent_allowlist_shared
                            .lock()
                            .await
                            .insert(key.clone());
                    }
                }
            }
        } else {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
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
                name_in,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FunctionCall;

    fn tc(name: &str) -> ToolCall {
        ToolCall {
            id: "x".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    #[test]
    fn parallel_sync_batch_two_readonly_sync_tools() {
        let batch = vec![tc("read_file"), tc("list_dir")];
        assert!(tool_calls_allow_parallel_sync_batch(&batch));
    }

    #[test]
    fn parallel_sync_batch_mixed_readonly_http_and_search() {
        assert!(tool_calls_allow_parallel_sync_batch(&[
            tc("read_file"),
            tc("http_fetch")
        ]));
        assert!(tool_calls_allow_parallel_sync_batch(&[
            tc("get_weather"),
            tc("web_search")
        ]));
    }

    #[test]
    fn parallel_sync_batch_denied_for_cargo_or_workflow() {
        assert!(!tool_calls_allow_parallel_sync_batch(&[
            tc("read_file"),
            tc("cargo_check")
        ]));
        assert!(!tool_calls_allow_parallel_sync_batch(&[
            tc("workflow_execute"),
            tc("read_file")
        ]));
    }

    #[test]
    fn parallel_sync_batch_denied_for_http_request() {
        assert!(!tool_calls_allow_parallel_sync_batch(&[
            tc("read_file"),
            tc("http_request")
        ]));
    }

    #[test]
    fn parallel_sync_batch_single_tool_false() {
        assert!(!tool_calls_allow_parallel_sync_batch(&[tc("read_file")]));
    }

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
    fn sync_default_inline_tools() {
        assert!(sync_default_runs_inline("get_current_time"));
        assert!(sync_default_runs_inline("convert_units"));
        assert!(!sync_default_runs_inline("read_file"));
        assert!(!sync_default_runs_inline("calc"));
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
