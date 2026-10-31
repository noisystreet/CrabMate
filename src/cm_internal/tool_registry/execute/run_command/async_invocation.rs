//! `run_command` 流水线的 后台任务（async）调用 阶段。
//!
//! 原为 `execute/execute_run_command_async_invocation.inc.rs`，通过 `include!` 与其余六片共享
//! `execute` 的单一作用域；现为真子模块，跨片调用必须经 `mod.rs` 的显式接口（#1196）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_internal::tool_approval::{self, InteractiveGateOutcome};
use crate::cm_internal::tools;
use crate::cm_types::{CommandApprovalDecision, ToolCall};

// 祖先层（`execute`）的项：子模块可见祖先的私有项，故直接 glob 取用。
use super::super::*;
// 同一条流水线的兄弟子模块：只能看到 `mod.rs` 里 `pub(in crate::cm_internal::tool_registry::execute) use` 出来的显式接口。
use super::*;
use crate::cm_names as tool_names;

pub(in crate::cm_internal::tool_registry::execute) struct RunCommandAsyncInvoke<'a> {
    pub(in crate::cm_internal::tool_registry::execute) env: &'a ToolExecEnv<'a>,
    pub(in crate::cm_internal::tool_registry::execute) effective_working_dir: &'a Path,
    pub(in crate::cm_internal::tool_registry::execute) web_ctx: Option<&'a WebToolRuntime>,
    pub(in crate::cm_internal::tool_registry::execute) args: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) tool_jobs: std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
    pub(in crate::cm_internal::tool_registry::execute) parsed: ParsedRunCommandJson,
    pub(in crate::cm_internal::tool_registry::execute) decision: PolicyDecision<'a>,
    pub(in crate::cm_internal::tool_registry::execute) timeout_secs: Option<u64>,
}

/// `run_command` 的 `async=true` 后台路径（契约 §1.2 / §2）。
///
/// 前置检查、解析与审批（`async_mode=true` 拒绝一切交互审批）均由 [`execute_run_command_impl`]
/// 完成；此处仅构造进程组可执行参数、登记后台任务并立即返回启动帧。
/// 轮询/取消走 `GET /tools/jobs/{id}` 等端点。
pub(in crate::cm_internal::tool_registry::execute) async fn execute_run_command_async(
    invoke: RunCommandAsyncInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandAsyncInvoke {
        env,
        effective_working_dir,
        web_ctx,
        args,
        tool_jobs: registry,
        parsed,
        decision,
        timeout_secs,
    } = invoke;
    let cfg = env.cfg;
    let effective_allowed_arc = Arc::clone(&decision.caps.allowed_commands);
    let skip_arg_safety = decision.skip_arg_safety;

    let prepared =
        match tools::prepare_run_command_for_pty_spawn_spec(
            &parsed.spec,
            effective_working_dir,
            effective_allowed_arc.as_ref(),
            skip_arg_safety,
        ) {
            Ok(p) => p,
            Err(e) => return (e.extended_user_message(), None),
        };
    let program = match prepared.exec_path {
        Some(p) => p.to_string_lossy().into_owned(),
        None => prepared.cmd_name,
    };
    // gh token：发起时刻解析并固化到 spawn（worker 线程无请求作用域）。
    let extra_env = if crate::cm_tools::github_token::command_basename_is_gh(&program) {
        crate::cm_tools::github_token::resolve_token_for_child_env()
            .map(|t| vec![("GH_TOKEN".to_string(), t)])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let wall = Duration::from_secs(
        timeout_secs
            .map(|t| t.clamp(1, 600))
            .unwrap_or(cfg.command_exec.command_timeout_secs)
            .max(1),
    );
    let spawn = crate::cm_internal::tool_jobs::JobSpawn {
        program,
        args: prepared.cmd_args,
        cwd: prepared.effective_working_dir,
        extra_env,
        wall,
        max_output_len: cfg.command_exec.command_max_output_len,
    };
    // 终态补发与启动帧走通用发起（`launch_background_job`），与其它装配工具完全同构。
    launch_background_job(
        &registry,
        web_ctx,
        tool_names::RUN_COMMAND,
        args,
        effective_working_dir,
        spawn,
    )
}
