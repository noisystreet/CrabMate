//! `run_command` 流水线的 执行入口与能力快照 阶段。
//!
//! 原为 `execute/execute_run_command_facade.inc.rs`，通过 `include!` 与其余六片共享
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

/// 执行层能力快照：一次 `run_command` 调用进入 façade 前即可判定的执行条件。
///
/// 与 [`PolicyCapabilities`] 的区别：后者只服务审批（是否有审批通道、是否 async/Docker），
/// 本结构额外携带后台任务装配信息，供 façade 分派与前置检查复用。
#[derive(Debug, Clone, Copy)]
pub(in crate::cm_internal::tool_registry::execute) struct RunCommandCapabilities {
    pub(in crate::cm_internal::tool_registry::execute) mode: ExecMode,
    pub(in crate::cm_internal::tool_registry::execute) background_jobs_enabled: bool,
    pub(in crate::cm_internal::tool_registry::execute) has_job_registry: bool,
    /// 与 `mode` 仅在 Async+Docker 组合下不同；该组合已被 [`Self::async_unavailable_reason`]
    /// 前置拒绝，此处保留布尔值是供宿主入口在 Sync/Docker 共用路径中查表。
    pub(in crate::cm_internal::tool_registry::execute) docker_sandbox: bool,
}

impl RunCommandCapabilities {
    /// `async` 前置检查；判定顺序与拆分前逐字一致（开关 → 注册表 → Docker）。
    pub(in crate::cm_internal::tool_registry::execute) fn async_unavailable_reason(&self) -> Option<&'static str> {
        if !self.background_jobs_enabled {
            Some(
                "错误：后台任务（async）未启用（[tool_registry] background_jobs_enabled=false）。请启用后台任务或去掉 async 参数。",
            )
        } else if !self.has_job_registry {
            Some("错误：当前执行环境不支持后台任务（async）。请去掉 async 参数。")
        } else if self.docker_sandbox {
            Some(
                "错误：后台任务（async）暂不支持 Docker 同步工具沙盒模式；请使用宿主模式或去掉 async 参数。",
            )
        } else {
            None
        }
    }
}

/// 单一执行 façade 的入参：解析与策略阶段已完成，只按 [`ExecMode`] 分派。
pub(in crate::cm_internal::tool_registry::execute) struct RunCommandFacadeInvoke<'a> {
    pub(in crate::cm_internal::tool_registry::execute) env: &'a ToolExecEnv<'a>,
    pub(in crate::cm_internal::tool_registry::execute) effective_working_dir: &'a Path,
    pub(in crate::cm_internal::tool_registry::execute) workspace_is_set: bool,
    pub(in crate::cm_internal::tool_registry::execute) workspace_changed: &'a mut bool,
    pub(in crate::cm_internal::tool_registry::execute) web_ctx: Option<&'a WebToolRuntime>,
    pub(in crate::cm_internal::tool_registry::execute) args: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) tool_call_id: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub(in crate::cm_internal::tool_registry::execute) sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    pub(in crate::cm_internal::tool_registry::execute) cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(in crate::cm_internal::tool_registry::execute) tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
    pub(in crate::cm_internal::tool_registry::execute) timeout_secs: Option<u64>,
    pub(in crate::cm_internal::tool_registry::execute) parsed: ParsedRunCommandJson,
    pub(in crate::cm_internal::tool_registry::execute) decision: PolicyDecision<'a>,
    pub(in crate::cm_internal::tool_registry::execute) capabilities: RunCommandCapabilities,
}

/// 单一执行 façade：按 [`ExecMode`] 分派到同步宿主 / 后台任务实现。
///
/// `Docker` 与 `Sync` 共用宿主入口（宿主入口内部按 [`RunCommandCapabilities::docker_sandbox`]
/// 决定是否转交容器 runner）；`Pty` 仅由 `terminal_session` 使用，此处防御性拒绝。
pub(in crate::cm_internal::tool_registry::execute) async fn execute_run_command_facade(
    invoke: RunCommandFacadeInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandFacadeInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
        tool_jobs,
        timeout_secs,
        parsed,
        decision,
        capabilities,
    } = invoke;
    match capabilities.mode {
        ExecMode::Sync | ExecMode::Docker => {
            execute_run_command_sync_host(RunCommandSyncHostInvoke {
                env,
                effective_working_dir,
                workspace_is_set,
                workspace_changed,
                args,
                tool_call_id,
                sse_out_tx,
                sse_control_mirror,
                cancel,
                timeout_secs,
                parsed,
                decision,
                capabilities,
            })
            .await
        }
        ExecMode::Async => {
            // 防御分支：正常装配路径已在 `async_unavailable_reason()` 拦截无注册表的情况；
            // 保留兜底，避免未来新增能力快照构造方时绕过前置检查。
            let Some(registry) = tool_jobs else {
                return (
                    "错误：当前执行环境不支持后台任务（async）。请去掉 async 参数。".to_string(),
                    None,
                );
            };
            execute_run_command_async(RunCommandAsyncInvoke {
                env,
                effective_working_dir,
                web_ctx,
                args,
                tool_jobs: registry,
                parsed,
                decision,
                timeout_secs,
            })
            .await
        }
        ExecMode::Pty => (
            "错误：run_command 不支持 PTY 执行模式。".to_string(),
            None,
        ),
    }
}
