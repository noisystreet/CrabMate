//! 工具分发注册表：按工具名解析执行策略（workflow / 阻塞+超时 / 同步），Web 与 TUI 共用实现。
//!
//! **`spawn_blocking` 与配置**：进入阻塞池前对 [`AgentConfig`] 使用 [`Arc::clone`]（仅增引用计数），闭包内通过 [`tools::tool_context_for`] 借用同一份配置与白名单；`allowed_commands` 在 [`AgentConfig`] 内为 [`std::sync::Arc`] 共享切片，避免每轮工具调用整表克隆。纯 CPU、无阻塞 IO 的少数工具可走 [`sync_default_runs_inline`] 在当前 async 任务上直接执行。
//!
//! 新增「需特殊运行时」的工具：在下方 **`tool_dispatch_registry!`** 宏调用中增一行（`HandlerId` + `ToolDispatchMeta` 同源），并在 `dispatch_tool` 的 `match hid` 中补分支。

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

fn execution_class_parallel_wall_key(class: ToolExecutionClass) -> &'static str {
    match class {
        ToolExecutionClass::Workflow => "workflow",
        ToolExecutionClass::CommandSpawnTimeout => "command_spawn_timeout",
        ToolExecutionClass::ExecutableSpawnTimeout => "executable_spawn_timeout",
        ToolExecutionClass::WeatherSpawnTimeout => "weather_spawn_timeout",
        ToolExecutionClass::WebSearchSpawnTimeout => "web_search_spawn_timeout",
        ToolExecutionClass::HttpFetchSpawnTimeout => "http_fetch_spawn_timeout",
        ToolExecutionClass::BlockingSync => "blocking_sync",
    }
}

/// 并行只读批与 **`SyncDefault` + `spawn_blocking`** 路径共用的墙上时钟上限（秒），与各 `execute_*_web` 中 **`tokio::time::timeout`** 一致，避免批内工具无限阻塞。
///
/// 可由 **`[tool_registry] parallel_wall_timeout_secs`** 按执行类键覆盖（见 `config/tools.toml`）。
pub fn parallel_tool_wall_timeout_secs(cfg: &AgentConfig, tool_name: &str) -> u64 {
    let class = execution_class_for_tool(tool_name);
    let key = execution_class_parallel_wall_key(class);
    if let Some(&secs) = cfg.tool_registry_parallel_wall_timeout_secs.get(key) {
        return secs.max(1);
    }
    use ToolExecutionClass::*;
    match class {
        HttpFetchSpawnTimeout => cfg
            .http_fetch_timeout_secs
            .max(1)
            .max(cfg.command_timeout_secs.max(1)),
        WeatherSpawnTimeout => cfg.weather_timeout_secs.max(1),
        WebSearchSpawnTimeout => cfg.web_search_timeout_secs.max(1),
        CommandSpawnTimeout | ExecutableSpawnTimeout => cfg.command_timeout_secs.max(1),
        Workflow | BlockingSync => cfg.command_timeout_secs.max(1),
    }
}

/// `http_fetch` / `http_request`：`spawn_blocking` **外圈** `tokio::time::timeout`（与 `reqwest` 内读秒数 `http_fetch_timeout_secs` 区分）。
pub(crate) fn http_fetch_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_http_fetch_wall_timeout_secs
        .unwrap_or_else(|| {
            cfg.command_timeout_secs
                .max(cfg.http_fetch_timeout_secs)
                .max(1)
        })
}

pub(crate) fn http_request_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_http_request_wall_timeout_secs
        .unwrap_or_else(|| http_fetch_outer_wall_secs(cfg))
}

fn builtin_write_effect_tools() -> &'static HashSet<String> {
    static W: OnceLock<HashSet<String>> = OnceLock::new();
    W.get_or_init(|| {
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
            "docker_build",
            "run_command",
            "playbook_run_commands",
            "run_executable",
            "workflow_execute",
            "http_request",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    })
}

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
/// 写操作工具（create/modify/delete/move/copy/format/apply_patch 等）及带审批的工具返回 false。
///
/// 写工具名表可由 **`[tool_registry] write_effect_tools`** 整表覆盖。
pub fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    if crate::mcp::is_mcp_proxy_tool(name) {
        // 外部 MCP 工具语义未知，禁止与内建只读工具并行同批执行。
        return false;
    }
    let writes = match &cfg.tool_registry_write_effect_tools {
        None => builtin_write_effect_tools(),
        Some(arc) => arc.as_ref(),
    };
    !writes.contains(name)
}

fn builtin_parallel_sync_denied_exact() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "rust_compiler_json",
            "quality_workspace",
            "ci_pipeline_local",
            "repo_overview_sweep",
            "codebase_semantic_search",
            "docs_health_sweep",
            "playbook_run_commands",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    })
}

fn builtin_parallel_sync_prefix_hit(name: &str) -> bool {
    name.starts_with("cargo_")
        || name.starts_with("npm_")
        || name.starts_with("frontend_")
        || name.starts_with("go_")
        || name.starts_with("maven_")
        || name.starts_with("gradle_")
        || name.starts_with("docker_")
        || name.starts_with("podman_")
        || name.starts_with("ruff_")
        || name.starts_with("pytest")
        || name.starts_with("mypy_")
        || name.starts_with("uv_")
        || name.starts_with("pre_commit")
        || name.starts_with("python_")
        || name.starts_with("typos_")
        || name.starts_with("codespell_")
}

/// 即使 [`is_readonly_tool`] 为真，并行 `spawn_blocking` 仍可能争抢 cargo/npm 等构建锁或缓存；勿与同批其它工具并行。
fn parallel_sync_batch_denied(cfg: &AgentConfig, name: &str) -> bool {
    let exact = match &cfg.tool_registry_parallel_sync_denied_tools {
        None => builtin_parallel_sync_denied_exact(),
        Some(arc) => arc.as_ref(),
    };
    if exact.contains(name) {
        return true;
    }
    match &cfg.tool_registry_parallel_sync_denied_prefixes {
        None => builtin_parallel_sync_prefix_hit(name),
        Some(prefs) => prefs.iter().any(|p| name.starts_with(p)),
    }
}

/// 可与其它只读工具同批 **并行** 执行的工具（不含 `http_request`、命令类、MCP）。
///
/// - **`SyncDefault`**：内建只读且非 `parallel_sync_batch_denied`。
/// - **`http_fetch`**：GET/HEAD 只读；审批在并行 `spawn_blocking` 之前**串行**完成（见 `execute_tools`）。
/// - **`get_weather` / `web_search`**：出站只读 HTTP；无工作区副作用，可与 `read_file` 等同批并行。
fn parallel_batch_eligible_tool(cfg: &AgentConfig, name: &str) -> bool {
    if parallel_sync_batch_denied(cfg, name) {
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
pub fn tool_ok_for_parallel_readonly_batch_piece(cfg: &AgentConfig, name: &str) -> bool {
    !crate::mcp::is_mcp_proxy_tool(name)
        && is_readonly_tool(cfg, name)
        && parallel_batch_eligible_tool(cfg, name)
}

/// 本批 **至少 2 个** 工具且全部为语义只读、且均为 [`parallel_batch_eligible_tool`] 时，可在单轮内并行执行
///（`SyncDefault` / `http_fetch` / `get_weather` / `web_search`；**不含** `http_request`、命令类、MCP；`http_fetch` 的审批先于并行 IO，见 `agent_turn::per_execute_tools_common`）。
pub fn tool_calls_allow_parallel_sync_batch(cfg: &AgentConfig, tool_calls: &[ToolCall]) -> bool {
    tool_calls.len() > 1
        && tool_calls
            .iter()
            .all(|tc| tool_ok_for_parallel_readonly_batch_piece(cfg, tc.function.name.as_str()))
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

impl WebToolRuntime {
    pub(crate) fn approval_sink(&self) -> crate::tool_approval::WebApprovalSink<'_> {
        crate::tool_approval::WebApprovalSink {
            out_tx: &self.out_tx,
            approval_rx_shared: &self.approval_rx_shared,
            approval_request_guard: &self.approval_request_guard,
        }
    }
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
    /// `--yes`：对 [`crate::tool_approval::SensitiveCapability`] 所覆盖的敏感工具（`run_command`、未匹配前缀的 `http_fetch` / `http_request` 等）在非白名单时也自动「本次允许」（**仅可信环境**；与 [`crate::tool_approval::CliApprovalInput`] 同源语义）。
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

/// 由 `tool_dispatch_registry!` 展开：生成 `DISPATCH_METADATA` 与 `handler_dispatch_map_build`，与 `HANDLER_MAP` 同源。
macro_rules! tool_dispatch_registry {
    ( $( ( $name:literal, $reqws:expr, $class:ident, $handler:ident ) ),* $(,)? ) => {
        static DISPATCH_METADATA: &[ToolDispatchMeta] = &[
            $(
                ToolDispatchMeta {
                    name: $name,
                    requires_workspace: $reqws,
                    class: ToolExecutionClass::$class,
                },
            )*
        ];

        fn handler_dispatch_map_build() -> HashMap<&'static str, HandlerId> {
            let mut m = HashMap::new();
            $(
                m.insert($name, HandlerId::$handler);
            )*
            m
        }
    };
}

tool_dispatch_registry! {
    ("workflow_execute", false, Workflow, Workflow),
    ("run_command", true, CommandSpawnTimeout, RunCommand),
    ("run_executable", true, ExecutableSpawnTimeout, RunExecutable),
    ("get_weather", false, WeatherSpawnTimeout, GetWeather),
    ("web_search", false, WebSearchSpawnTimeout, WebSearch),
    ("http_fetch", false, HttpFetchSpawnTimeout, HttpFetch),
    ("http_request", false, HttpFetchSpawnTimeout, HttpRequest),
}

/// 注册表中显式声明的工具；其余名称运行时走 `SyncDefault`（同步 `run_tool`）。
/// 与 `handler_id_for` / `HANDLER_MAP` 共用 `tool_dispatch_registry!` 生成的表，勿分开维护。
pub fn all_dispatch_metadata() -> &'static [ToolDispatchMeta] {
    DISPATCH_METADATA
}

static HANDLER_MAP: OnceLock<HashMap<&'static str, HandlerId>> = OnceLock::new();

fn builtin_sync_default_inline_tools() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| {
        ["get_current_time", "convert_units"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    })
}

/// 无子进程、无阻塞网络/磁盘的 `SyncDefault` 工具：跳过 `spawn_blocking`，以免线程池调度开销大于工具本身。
///
/// 可由 **`[tool_registry] sync_default_inline_tools`** 覆盖。
pub(crate) fn sync_default_runs_inline(cfg: &AgentConfig, name: &str) -> bool {
    match &cfg.tool_registry_sync_default_inline_tools {
        None => builtin_sync_default_inline_tools().contains(name),
        Some(arc) => arc.contains(name),
    }
}

fn handler_id_for(name: &str) -> HandlerId {
    HANDLER_MAP
        .get_or_init(handler_dispatch_map_build)
        .get(name)
        .copied()
        .unwrap_or(HandlerId::SyncDefault)
}

/// [`dispatch_tool`] 入参（聚合 Web / CLI 统一上下文）。
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

    fn test_cfg() -> crate::config::AgentConfig {
        crate::config::load_config(None).expect("embed default")
    }

    #[test]
    fn parallel_sync_batch_two_readonly_sync_tools() {
        let cfg = test_cfg();
        let batch = vec![tc("read_file"), tc("list_dir")];
        assert!(tool_calls_allow_parallel_sync_batch(&cfg, &batch));
    }

    #[test]
    fn parallel_sync_batch_mixed_readonly_http_and_search() {
        let cfg = test_cfg();
        assert!(tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("read_file"), tc("http_fetch")]
        ));
        assert!(tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("get_weather"), tc("web_search")]
        ));
    }

    #[test]
    fn parallel_sync_batch_denied_for_cargo_or_workflow() {
        let cfg = test_cfg();
        assert!(!tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("read_file"), tc("cargo_check")]
        ));
        assert!(!tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("workflow_execute"), tc("read_file")]
        ));
    }

    #[test]
    fn parallel_sync_batch_denied_for_http_request() {
        let cfg = test_cfg();
        assert!(!tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("read_file"), tc("http_request")]
        ));
    }

    #[test]
    fn parallel_sync_batch_single_tool_false() {
        let cfg = test_cfg();
        assert!(!tool_calls_allow_parallel_sync_batch(
            &cfg,
            &[tc("read_file")]
        ));
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
        let cfg = test_cfg();
        assert!(sync_default_runs_inline(&cfg, "get_current_time"));
        assert!(sync_default_runs_inline(&cfg, "convert_units"));
        assert!(!sync_default_runs_inline(&cfg, "read_file"));
        assert!(!sync_default_runs_inline(&cfg, "calc"));
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

    #[test]
    fn parallel_tool_wall_timeout_secs_smoke() {
        let cfg = crate::config::load_config(None).expect("embed default");
        let cmd_budget = parallel_tool_wall_timeout_secs(&cfg, "read_file");
        assert!(cmd_budget >= 1);
        let fetch_budget = parallel_tool_wall_timeout_secs(&cfg, "http_fetch");
        assert!(fetch_budget >= cmd_budget);
        assert_eq!(
            parallel_tool_wall_timeout_secs(&cfg, "get_weather"),
            cfg.weather_timeout_secs.max(1)
        );
    }
}
