//! 工具分发编排层：`dispatch_tool` 重试外壳、单次分发与各 handler 分支。
//!
//! 原为 `execute_dispatch_body.inc.rs`，经 `include!` 与其余四片共享 `execute` 的**同一作用域**：
//! 漏改一片编译器也无从发现（#1158 / #1198）。现为真子模块，跨模块调用只能经 `mod.rs` 的
//! 显式接口清单。
//!
//! 本模块只做编排：具体执行分别落在 `background_job` / `http` / `terminal_session` /
//! `run_command`；动态工具、MCP 代理与 `SyncDefault` 分支在本文件内解析后转发。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use log::error;

use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_tools::tool_result::error_text;
use crate::cm_tools::tool_retry_policy::ToolRetrySpec;
use crate::cm_tools::tools;
use crate::cm_types::ToolCall;
use crate::tool_host::tool_approval::{self, InteractiveGateOutcome};

use super::super::meta::{HandlerId, HandlerLookupTable};
use super::super::policy::{parallel_tool_wall_timeout_secs, sync_default_runs_inline};
use super::super::runtime::ToolRuntime;
use super::run_command::{RunCommandHostInvoke, execute_run_command_impl};
// 祖先层（`execute`）的项，以及 `mod.rs` 里集中声明的兄弟子模块显式接口。
use super::*;

/// `HandlerId::SyncDefault` 分支入参（与 [`DispatchToolParams`] 中部分字段一致，避免 `dispatch_sync_default` 形参过多）。
struct SyncDefaultToolDispatchArgs<'a> {
    env: &'a ToolExecEnv<'a>,
    runtime: ToolRuntime<'a>,
    cfg: &'a Arc<AgentConfig>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    name: &'a str,
    args: &'a str,
    tc: &'a ToolCall,
    read_file_turn_cache:
        Option<std::sync::Arc<crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist:
        Option<std::sync::Arc<crate::cm_tools::workspace::changelist::WorkspaceChangelist>>,
    long_term_memory:
        Option<Arc<crate::cm_memory::memory::long_term_memory::LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    /// 后台任务注册表：供 `background_job_status` / `background_job_list` 只读查询。
    tool_jobs: Option<std::sync::Arc<crate::jobs::tool_jobs::ToolJobRegistry>>,
}

/// `http_fetch` / `http_request` 共用：可选 Web 审批会话（本路径不使用 `workspace_changed`）。
fn http_tool_approval_context<'a>(
    runtime: ToolRuntime<'a>,
) -> Option<&'a dyn crate::cm_approval::ApprovalBroker> {
    runtime.ctx
}

/// 检测 `read_dir` 入参中 `path` 是否为外部路径（绝对路径或含 `..`）。
pub(in crate::tool_host::tool_registry::execute) fn read_dir_path_is_external(
    args_json: &str,
) -> Option<String> {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let path = v.get("path")?.as_str()?.trim();
    if path.starts_with('/') || path.contains("..") {
        return Some(path.to_string());
    }
    None
}

pub(in crate::tool_host::tool_registry::execute) async fn approve_external_read_dir_if_needed(
    args: &str,
    web_ctx: Option<&dyn crate::cm_approval::ApprovalBroker>,
    permission: Option<&dyn crate::cm_agent::permission::PolicyEngine>,
    // `Some((工作目录, 允许根集合))` = 调用点能提供 S7 判定所需的上下文；
    // `None` = 只做 L4 / L2 定论（并行批预审批这一层拿不到工作目录与根集合）。
    scope_ctx: Option<(&std::path::Path, &[std::path::PathBuf])>,
) -> Result<(), String> {
    let Some(ext_path) = read_dir_path_is_external(args) else {
        return Ok(());
    };
    // S3：路径轴先定论——L4 放行（不弹窗），L2 硬拒（不弹窗），其余走现状审批。
    if let Some(caps) = permission {
        // 审计正文只留末两段路径（完整绝对路径不进日志）。
        let audit_detail = crate::cm_tools::tools::audit_path_detail(&ext_path);
        if caps.path_scope().is_unrestricted() {
            crate::cm_agent::permission_audit::record_allowed(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Path,
                Some("read_dir"),
                Some(&audit_detail),
            );
            return Ok(());
        }
        // S7：`AllowedRoots`（L2 / L3 + 非空 `workspace_allowed_roots`）下，落在
        // {会话根} ∪ 允许根内的目标直接放行——不弹窗、**不记 `allowed`**（`allowed` 只由
        // L4 的 `AuditOnly` 记，设计 §6.3）；越界仍走下面的 `Deny` / 交互审批。
        if caps.path_scope() == crate::cm_types::PathScope::AllowedRoots {
            let Some((working_dir, allowed_roots)) = scope_ctx else {
                // 预审批层拿不到工作目录 / 根集合：**不定论**，交给 `dispatch_tool` 里的
                // 正式判定。这里若继续往下走，L2 会把「落在允许根内」的目标误判成硬拒，
                // L3 会先多弹一次窗。
                return Ok(());
            };
            let within = crate::cm_tools::tools::PathAccess::with_scope_and_roots(
                working_dir,
                caps.path_scope(),
                allowed_roots,
            )
            .is_within_scope(&ext_path);
            if within {
                return Ok(());
            }
        }
        if caps.approval() == crate::cm_agent::permission::ApprovalPolicy::Deny {
            crate::cm_agent::permission_audit::record_denied(
                caps,
                crate::cm_agent::permission_audit::PermissionAxis::Path,
                Some("read_dir"),
                Some(&audit_detail),
            );
            return Err(format!(
                "错误：当前权限等级（{}）下不允许 read_dir 访问工作区外路径 \"{}\"：{}。",
                caps.level().as_str(),
                ext_path,
                caps.level().promise_zh()
            ));
        }
    }
    if web_ctx.is_none() {
        return Err(format!(
            "错误：read_dir 访问工作区外路径 \"{}\" 需要审批通道（当前无可用会话）。",
            ext_path
        ));
    }
    let spec = tool_approval::approval_spec_read_dir_external_path(&ext_path);
    let allow_handles =
        tool_approval::shared_allowlist_handles_web(web_ctx, permission.map(|c| c.level()));
    match tool_approval::interactive_gate_after_whitelist_miss(
        web_ctx.and_then(|w| w.approval_sink()),
        &spec,
        "tool_registry::read_dir external path approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => Ok(()),
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(format!("已拒绝：{}", msg)),
        Err(e) => Err(e.user_message_zh().to_string()),
    }
}

async fn try_dispatch_dynamic_tool(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    if crate::cm_tools::tool_naming::tool_origin(name)
        != crate::cm_tools::tool_naming::ToolOrigin::Dynamic
    {
        return None;
    }
    if !workspace_is_set {
        return Some((
            web_tool_err_workspace_not_set("执行动态工具").to_string(),
            None,
        ));
    }
    let def =
        match crate::tool_host::dynamic_tools::resolve_runtime_def(effective_working_dir, name) {
            Ok(Some(d)) => d,
            Ok(None) => {
                return Some((
                    format!("{}：{}", error_text::UNKNOWN_TOOL_PREFIX, name),
                    None,
                ));
            }
            Err(e) => return Some((format!("错误：动态工具加载失败：{}", e), None)),
        };
    let args_owned = args.to_string();
    let wd = effective_working_dir.to_path_buf();
    let cfg2 = Arc::clone(cfg);
    let wall_secs = cfg.command_exec.command_timeout_secs.max(1);
    let handle = tokio::task::spawn_blocking(move || {
        crate::tool_host::dynamic_tools::run_dynamic_tool(
            &def,
            &args_owned,
            wd.as_path(),
            cfg2.command_exec.command_max_output_len,
            cfg2.command_exec.allowed_commands.as_ref(),
        )
    });
    let out = match tokio::time::timeout(Duration::from_secs(wall_secs), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "动态工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("动态工具执行异常：{:?}", e)
        }
        Err(_) => format!("动态工具执行超时（{} 秒）", wall_secs),
    };
    Some((out, None))
}

async fn try_dispatch_mcp_proxy_tool(
    cfg: &Arc<AgentConfig>,
    mcp_turn: Option<&crate::mcp::McpTurnHandle>,
    name: &str,
    args: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    if crate::cm_tools::tool_naming::tool_origin(name)
        != crate::cm_tools::tool_naming::ToolOrigin::Mcp
    {
        return None;
    }
    let Some(turn) = mcp_turn else {
        return Some((
            "错误：MCP 会话未建立（连接或 tools/list 失败）".to_string(),
            None,
        ));
    };
    let Some((sess, remote)) = turn.session_for_openai_tool(name) else {
        return Some((
            "错误：无法将工具名解析为 MCP 远端名（请检查 MCP 服务器 slug 与命名前缀）".to_string(),
            None,
        ));
    };
    let guard = sess.lock().await;
    let mcp_args = crate::tool_host::tool_call_explain::strip_explain_why_if_present(args);
    let out = crate::mcp::call_mcp_tool(
        &guard,
        remote.as_str(),
        mcp_args.as_str(),
        Duration::from_secs(turn.tool_timeout_secs.max(1)),
        cfg.command_exec.command_max_output_len,
    )
    .await;
    Some((out, None))
}

async fn dispatch_sync_default_tool(p: SyncDefaultToolDispatchArgs<'_>) -> ToolRunOutcome {
    let SyncDefaultToolDispatchArgs {
        env,
        runtime,
        cfg,
        effective_working_dir,
        workspace_is_set,
        name,
        args,
        tc,
        read_file_turn_cache,
        workspace_changelist,
        long_term_memory,
        long_term_memory_scope_id,
        tool_jobs,
    } = p;
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        if !workspace_is_set {
            return ToolRunOutcome::failed(
                "错误：未设置工作区，无法在 Docker 沙盒中执行 SyncDefault 工具（请先设置工作区目录）。"
                    .to_string(),
                Some("workspace_not_set".to_string()),
            );
        }
        let out = crate::tool_sandbox::run_sync_default_in_docker(
            env.sandbox_backend,
            cfg.as_ref(),
            effective_working_dir,
            name,
            args,
        )
        .await;
        return match out {
            Ok(s) => ToolRunOutcome::ok(s, None),
            Err(e) => ToolRunOutcome::failed(e, None),
        };
    }

    // `read_dir` 外部路径审批：绝对路径或含 `..` 时需用户确认（不走白名单）。
    // 文件类工具的路径范围（S3）：只在 `dispatch` 这一处把权限投影落成 `ToolContext`，
    // 各工具实现只读 `ctx.path_scope`。
    let path_scope = env
        .permission
        .map(|c| c.path_scope())
        .unwrap_or(crate::cm_types::PathScope::SessionRoot);
    // S7：根集合只在这一处注入；`AllowedRoots` 档由 `PathAccess` 用它选解析基，
    // `SessionRoot` / `Any` 档完全忽略它（空切片即现状）。
    let allowed_roots = cfg
        .as_ref()
        .workspace_roots
        .workspace_allowed_roots
        .as_slice();

    if name == crate::cm_names::READ_DIR {
        let web_ctx = http_tool_approval_context(runtime);
        if let Err(msg) = approve_external_read_dir_if_needed(
            args,
            web_ctx,
            env.permission,
            Some((effective_working_dir, allowed_roots)),
        )
        .await
        {
            return ToolRunOutcome::failed(msg, None);
        }
    }

    if sync_default_runs_inline(cfg.as_ref(), name) {
        let hosts = crate::tool_host::memory_tool_hosts::DispatchMemoryHosts::from_dispatch_inputs(
            cfg.as_ref(),
            long_term_memory.clone(),
            long_term_memory_scope_id.as_deref(),
        );
        let jobs_host = tool_jobs
            .as_ref()
            .map(|r| crate::tool_host::memory_tool_hosts::ToolJobsHost::new(Arc::clone(r)));
        let ctx = tools::tool_context_for_with_read_cache_and_memory(
            cfg.as_ref(),
            cfg.command_exec.allowed_commands.as_ref(),
            effective_working_dir,
            read_file_turn_cache.as_ref().map(|a| a.as_ref()),
            workspace_changelist.as_ref(),
            Some(hosts.codebase_ref()),
            hosts.long_term_ref(),
            jobs_host
                .as_ref()
                .map(|h| h as &dyn crate::cm_tools::memory_tool_host::ToolJobsToolHost),
        )
        .with_path_scope_and_roots(path_scope, allowed_roots);
        let (output, ok, error_code) = tools::run_tool_outcome(name, args, &ctx);
        return ToolRunOutcome {
            output,
            payload: None,
            ok,
            error_code,
        };
    }
    let cfg2 = Arc::clone(cfg);
    let tool_name = tc.function.name.clone();
    let tool_args = tc.function.arguments.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let rfc = read_file_turn_cache.clone();
    let wcl = workspace_changelist.clone();
    let ltm2 = long_term_memory.clone();
    let ltm_scope2 = long_term_memory_scope_id.clone();
    let tj = tool_jobs.clone();
    let wall_secs = parallel_tool_wall_timeout_secs(cfg.as_ref(), name);
    let github_token = crate::cm_tools::github_token::resolve_token_plaintext();
    let handle = tokio::task::spawn_blocking(move || {
        crate::cm_tools::github_token::with_request_github_token_blocking(github_token, || {
            let hosts =
                crate::tool_host::memory_tool_hosts::DispatchMemoryHosts::from_dispatch_inputs(
                    cfg2.as_ref(),
                    ltm2,
                    ltm_scope2.as_deref(),
                );
            let jobs_host = tj
                .as_ref()
                .map(|r| crate::tool_host::memory_tool_hosts::ToolJobsHost::new(Arc::clone(r)));
            let ctx = tools::tool_context_for_with_read_cache_and_memory(
                cfg2.as_ref(),
                cfg2.command_exec.allowed_commands.as_ref(),
                work_dir.as_path(),
                rfc.as_ref().map(|a| a.as_ref()),
                wcl.as_ref(),
                Some(hosts.codebase_ref()),
                hosts.long_term_ref(),
                jobs_host
                    .as_ref()
                    .map(|h| h as &dyn crate::cm_tools::memory_tool_host::ToolJobsToolHost),
            )
            .with_path_scope_and_roots(
                path_scope,
                cfg2.workspace_roots.workspace_allowed_roots.as_slice(),
            );
            tools::run_tool_outcome(&tool_name, &tool_args, &ctx)
        })
    });
    match tokio::time::timeout(Duration::from_secs(wall_secs), handle).await {
        Ok(Ok((output, ok, error_code))) => ToolRunOutcome {
            output,
            payload: None,
            ok,
            error_code,
        },
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            // 不给码：panic 没有既登记码，落 `{tool}_failed` 兜底比伪造一个未分类码更诚实。
            ToolRunOutcome::failed(
                format!("{}：工具执行异常：{:?}", error_text::ERROR_PREFIX, e),
                None,
            )
        }
        Err(_) => {
            error!(target: "crabmate", "工具执行超时 tool={} wall_secs={}", name, wall_secs);
            ToolRunOutcome::failed(
                format!(
                    "{}：工具执行超时（{} 秒）",
                    error_text::ERROR_PREFIX,
                    wall_secs
                ),
                Some("timeout".to_string()),
            )
        }
    }
}

/// 工具分发入口：默认单次执行；开启 `[tool_registry] tool_retry_*` 且资格门通过时，
/// 对瞬时类失败（`error_code ∈ tool_retry_error_codes`）做**透明重试**——失败中间态不写
/// 模型上下文，仅最终结果返回；重试间隔按 `backoff_for_retry` 指数退避，用户取消立即停止。
pub async fn dispatch_tool(mut p: DispatchToolParams<'_>) -> ToolRunOutcome {
    let spec = ToolRetrySpec::from_config(p.policy.cfg.as_ref());
    if !spec.tool_retry_eligible(p.policy.cfg.as_ref(), p.call.name, p.call.args) {
        return dispatch_tool_inner(&mut p).await;
    }
    let max_attempts = spec.max_attempts;
    let mut attempt: u64 = 1;
    loop {
        let outcome = dispatch_tool_inner(&mut p).await;
        if !spec.should_retry(outcome.error_code.as_deref(), attempt) {
            return outcome;
        }
        // 用户取消：停止重试，返回最后一次结果。
        if p.obs
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        {
            return outcome;
        }
        log::info!(
            target: "crabmate",
            "tool_retry tool={} attempt={}/{} error_code={} args_preview={}",
            p.call.name,
            attempt,
            max_attempts,
            outcome.error_code.as_deref().unwrap_or("-"),
            crate::cm_tools::redact::tool_arguments_preview_for_log(p.call.args)
        );
        let backoff = spec.backoff_for_retry(attempt);
        if backoff > 0 {
            let cancel = p.obs.cancel.clone();
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(backoff)) => {}
                _ = wait_for_tool_cancel(cancel.as_ref()) => {
                    // 退避期间用户取消：立即停止，返回最后一次结果。
                    return outcome;
                }
            }
        }
        attempt += 1;
    }
}

/// 等待用户取消标志置位（供重试退避期间响应取消）；`None` 表示无取消源，永久等待。
async fn wait_for_tool_cancel(cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>) {
    let Some(c) = cancel else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        if c.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// `workflow_execute` / `delegate_task` 误入 `dispatch_tool` 时的统一错误正文。
///
/// 带 `错误：` 前缀是 #1430 的要求：这条正文不经 `ToolError`，没有前缀就会被
/// ④ 级判成成功。
fn internal_dispatch_error(tool: &str) -> String {
    format!(
        "{}：内部错误：{tool} 须由 agent 层调度，不应进入 dispatch_tool",
        error_text::ERROR_PREFIX
    )
}

/// 分派前的三条短路路径：回合准入（拒绝即出结论）→ 动态工具 → MCP 代理。
///
/// 返回 `Some` 表示已得出结论、调用方直接返回；`None` 表示继续走内置 `HandlerId` 分派。
/// 提取自 `dispatch_tool_inner`：那里只留一次调用，避免准入/短路逻辑把主体撑过 nloc 棘轮。
async fn try_short_circuit_dispatch(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    turn_allow: Option<&HashSet<String>>,
    mcp_turn: Option<&crate::mcp::McpTurnHandle>,
    name: &str,
    args: &str,
) -> Option<ToolRunOutcome> {
    if !crate::context::agent_role_turn::tool_allowed_for_turn(name, turn_allow) {
        // 不给码：`tool_denied` 类文案已带「错误：」前缀，下游 `{tool}_failed` 兜底即可；
        // 且策略拒绝的既有码是 `command_denied` / `approval_denied`，不是分类名 `policy_denied`。
        return Some(ToolRunOutcome::failed(
            crate::context::agent_role_turn::turn_tool_denied_message(name),
            None,
        ));
    }

    if let Some((out, payload)) =
        try_dispatch_dynamic_tool(cfg, effective_working_dir, workspace_is_set, name, args).await
    {
        return Some(ToolRunOutcome::from_body_and_payload(name, out, payload));
    }

    let (out, payload) = try_dispatch_mcp_proxy_tool(cfg, mcp_turn, name, args).await?;
    Some(ToolRunOutcome::from_body_and_payload(name, out, payload))
}

/// Web 未设置工作区时：仍允许出网类工具；禁止所有 `SyncDefault`（否则 `working_dir` 会回落到
/// 配置目录，等效于未选工作区仍可读本地树）。返回 `Some` 表示应直接以此结论结束。
fn deny_sync_default_without_workspace(
    hid: HandlerId,
    workspace_is_set: bool,
) -> Option<ToolRunOutcome> {
    if workspace_is_set || !matches!(hid, HandlerId::SyncDefault) {
        return None;
    }
    Some(ToolRunOutcome::failed(
        web_tool_err_workspace_not_set("执行内置工具"),
        Some("workspace_not_set".to_string()),
    ))
}

/// 单次工具执行（含审批、沙盒与 handler 分发）；供 [`dispatch_tool`] 重试循环按可变借用复用。
async fn dispatch_tool_inner(p: &mut DispatchToolParams<'_>) -> ToolRunOutcome {
    let DispatchToolParams {
        runtime,
        call,
        workspace,
        policy,
        obs,
        memory,
        delegation: _,
    } = p;
    let DispatchToolCall { name, args, tc } = call;
    let DispatchToolWorkspace {
        effective_working_dir,
        workspace_is_set,
        workspace_changelist,
    } = workspace;
    let DispatchToolPolicy {
        cfg,
        turn_allow,
        handler_lookup,
        sync_default_sandbox_backend,
        permission,
    } = policy;
    let DispatchToolObs {
        sse_out_tx,
        sse_control_mirror,
        cancel,
        tool_jobs,
    } = obs;
    let DispatchToolMemory {
        read_file_turn_cache,
        long_term_memory,
        long_term_memory_scope_id,
        mcp_turn,
    } = memory;
    // 可变解构后的字段为 `&mut T` / `&mut &T`，经类型注解自动解引用/克隆为原类型，保持主体逻辑不变。
    let name: &str = name;
    let args: &str = args;
    let tc: &ToolCall = tc;
    let effective_working_dir: &Path = effective_working_dir;
    let workspace_is_set = *workspace_is_set;
    let workspace_changelist = workspace_changelist.clone();
    let cfg: &Arc<AgentConfig> = cfg;
    let turn_allow: Option<&HashSet<String>> = *turn_allow;
    let permission: Option<&dyn crate::cm_agent::permission::PolicyEngine> = *permission;
    let sse_out_tx = *sse_out_tx;
    let sse_control_mirror = *sse_control_mirror;
    let cancel = cancel.clone();
    let tool_jobs = tool_jobs.clone();
    let read_file_turn_cache = read_file_turn_cache.clone();
    let long_term_memory = long_term_memory.clone();
    let long_term_memory_scope_id = long_term_memory_scope_id.clone();
    let mcp_turn = *mcp_turn;
    // `&mut bool` 字段在可变借用下可移动重借用为 owned `ToolRuntime`。
    let runtime = ToolRuntime {
        workspace_changed: runtime.workspace_changed,
        ctx: runtime.ctx,
    };
    let env = ToolExecEnv {
        cfg,
        sandbox_backend: sync_default_sandbox_backend,
        permission,
    };
    if let Some(outcome) = try_short_circuit_dispatch(
        cfg,
        effective_working_dir,
        workspace_is_set,
        turn_allow,
        mcp_turn,
        name,
        args,
    )
    .await
    {
        return outcome;
    }

    let args_processed = match crate::tool_host::tool_call_explain::require_explain_for_mutation(
        cfg.as_ref(),
        name,
        args,
    ) {
        Ok(c) => c,
        // 不给码：`require_explain_for_mutation` 只返回用户可见文案，本来就没有错误码。
        Err(e) => return ToolRunOutcome::failed(e, None),
    };
    let args = args_processed.as_ref();

    let hid = handler_lookup.id_for(name);

    if let Some(outcome) = deny_sync_default_without_workspace(hid, workspace_is_set) {
        return outcome;
    }

    // 非 `run_command` 工具的 `async=true` 后台门闩（装配表内的进程载荷工具）。
    if let Some((out, payload)) = try_dispatch_background_async_tool(
        cfg,
        effective_working_dir,
        runtime.ctx,
        name,
        args,
        tool_jobs.as_ref(),
    ) {
        return ToolRunOutcome::from_body_and_payload(name, out, payload);
    }

    // 过渡包装：尚未改造的 `execute_*_impl` 仍返回 `(正文, 载荷)`，按其自带退出码 / 头折叠。
    // 已返回 `ToolRunOutcome` 的路径（`SyncDefault`）**不要**过这里，否则结论会被拍平再反推。
    let wrap = |parts: (String, Option<serde_json::Value>)| {
        ToolRunOutcome::from_body_and_payload(name, parts.0, parts.1)
    };
    match hid {
        HandlerId::Workflow => {
            // `workflow_execute` 由 `agent::workflow_tool_dispatch` 调度（避免本模块依赖 `PerCoordinator` / `workflow`）。
            ToolRunOutcome::failed(internal_dispatch_error("workflow_execute"), None)
        }
        HandlerId::Delegate => {
            // `delegate_task` 同理：它要 `SubAgentDelegator` 能力对象，而工具层**刻意不持 LLM 凭据**
            // （`agent_turn/host/execute/**` 中 `api_key` 零命中）。见 `docs/design/subagent_delegation.md` §3。
            ToolRunOutcome::failed(internal_dispatch_error("delegate_task"), None)
        }
        HandlerId::RunCommand => {
            let ToolRuntime {
                workspace_changed,
                ctx,
            } = runtime;
            wrap(
                execute_run_command_impl(RunCommandHostInvoke {
                    env: &env,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
                    web_ctx: ctx,
                    args,
                    tool_call_id: tc.id.as_str(),
                    sse_out_tx,
                    sse_control_mirror,
                    cancel,
                    tool_jobs,
                })
                .await,
            )
        }
        HandlerId::TerminalSession => {
            let ToolRuntime {
                workspace_changed,
                ctx,
            } = runtime;
            wrap(
                execute_terminal_session_impl(TerminalSessionExecInvoke {
                    env: &env,
                    effective_working_dir,
                    workspace_is_set,
                    workspace_changed,
                    web_ctx: ctx,
                    args,
                    sse_out_tx,
                    sse_control_mirror,
                    tool_call_id: tc.id.as_str(),
                    sse_encoder: None,
                    cancel,
                })
                .await,
            )
        }
        HandlerId::GetWeather => wrap(
            execute_get_weather_web(&env, effective_working_dir, workspace_is_set, name, args)
                .await,
        ),
        HandlerId::WebSearch => wrap(
            execute_web_search_web(&env, effective_working_dir, workspace_is_set, name, args).await,
        ),
        HandlerId::HttpFetch => wrap(
            execute_http_fetch_impl(
                &env,
                effective_working_dir,
                workspace_is_set,
                http_tool_approval_context(runtime),
                name,
                args,
            )
            .await,
        ),
        HandlerId::HttpRequest => wrap(
            execute_http_request_impl(
                &env,
                effective_working_dir,
                workspace_is_set,
                http_tool_approval_context(runtime),
                name,
                args,
            )
            .await,
        ),
        HandlerId::SyncDefault => {
            // 已返回 `ToolRunOutcome`：**不要**再折成 `(正文, 载荷)` 反推，否则结构化结论白丢。
            dispatch_sync_default_tool(SyncDefaultToolDispatchArgs {
                env: &env,
                runtime,
                cfg,
                effective_working_dir,
                workspace_is_set,
                name,
                args,
                tc,
                read_file_turn_cache,
                workspace_changelist,
                long_term_memory,
                long_term_memory_scope_id,
                tool_jobs,
            })
            .await
        }
    }
}

/// 并行只读批内 `SyncDefault` 预审批：当前仅覆盖 `read_dir` 的工作区外路径访问。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或无需审批。
pub async fn prefetch_parallel_syncdefault_approvals(
    tool_calls: &[ToolCall],
    web_ctx: Option<&dyn crate::cm_approval::ApprovalBroker>,
    handler_lookup: &HandlerLookupTable,
    permission: Option<&dyn crate::cm_agent::permission::PolicyEngine>,
) -> HashMap<(String, String), String> {
    let mut failures: HashMap<(String, String), String> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for tc in tool_calls {
        if handler_lookup.id_for(tc.function.name.as_str()) != HandlerId::SyncDefault {
            continue;
        }
        if tc.function.name != crate::cm_names::READ_DIR {
            continue;
        }
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Err(msg) = approve_external_read_dir_if_needed(
            tc.function.arguments.as_str(),
            web_ctx,
            permission,
            // 并行批预审批拿不到 `effective_working_dir` 与根集合；`AllowedRoots` 档
            // 在此**不定论**（`None`），交给下面每个工具自己的 `dispatch_tool`。
            None,
        )
        .await
        {
            failures.insert(key, msg);
        }
    }
    failures
}
