struct RunCommandAsyncInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    tool_jobs: std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
    parsed: ParsedRunCommandJson,
    decision: PolicyDecision,
    timeout_secs: Option<u64>,
}

/// `run_command` 的 `async=true` 后台路径（契约 §1.2 / §2）。
///
/// 前置检查、解析与审批（`async_mode=true` 拒绝一切交互审批）均由 [`execute_run_command_impl`]
/// 完成；此处仅构造进程组可执行参数、登记后台任务并立即返回启动帧。
/// 轮询/取消走 `GET /tools/jobs/{id}` 等端点。
async fn execute_run_command_async(
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
    let effective_allowed_arc = decision.allowed_commands;
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
        "run_command",
        args,
        effective_working_dir,
        spawn,
    )
}
