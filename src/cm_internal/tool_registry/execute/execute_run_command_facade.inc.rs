/// 执行层能力快照：一次 `run_command` 调用进入 façade 前即可判定的执行条件。
///
/// 与 [`PolicyCapabilities`] 的区别：后者只服务审批（是否有审批通道、是否 async/Docker），
/// 本结构额外携带后台任务装配信息，供 façade 分派与前置检查复用。
#[derive(Debug, Clone, Copy)]
struct RunCommandCapabilities {
    mode: ExecMode,
    background_jobs_enabled: bool,
    has_job_registry: bool,
    /// 与 `mode` 仅在 Async+Docker 组合下不同；该组合已被 [`Self::async_unavailable_reason`]
    /// 前置拒绝，此处保留布尔值是供宿主入口在 Sync/Docker 共用路径中查表。
    docker_sandbox: bool,
}

impl RunCommandCapabilities {
    /// `async` 前置检查；判定顺序与拆分前逐字一致（开关 → 注册表 → Docker）。
    fn async_unavailable_reason(&self) -> Option<&'static str> {
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
struct RunCommandFacadeInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
    timeout_secs: Option<u64>,
    parsed: ParsedRunCommandJson,
    decision: PolicyDecision<'a>,
    capabilities: RunCommandCapabilities,
}

/// 单一执行 façade：按 [`ExecMode`] 分派到同步宿主 / 后台任务实现。
///
/// `Docker` 与 `Sync` 共用宿主入口（宿主入口内部按 [`RunCommandCapabilities::docker_sandbox`]
/// 决定是否转交容器 runner）；`Pty` 仅由 `terminal_session` 使用，此处防御性拒绝。
async fn execute_run_command_facade(
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
