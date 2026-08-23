struct RunCommandAsyncInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

/// `run_command` 的 `async=true` 后台路径（契约 §1.2 / §2）。
///
/// 发起时刻完成白名单 / 路径 / 审批（`async_mode=true` 拒绝一切交互审批），
/// 构造进程组可执行参数后登记后台任务并立即返回启动帧；轮询/取消走 `GET /tools/jobs/{id}` 等端点。
async fn execute_run_command_async(
    invoke: RunCommandAsyncInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandAsyncInvoke {
        env,
        effective_working_dir,
        web_ctx,
        args,
        tool_jobs,
    } = invoke;
    let cfg = env.cfg;
    if !cfg.tool_registry_policy.tool_registry_background_jobs_enabled {
        return (
            "错误：后台任务（async）未启用（[tool_registry] background_jobs_enabled=false）。请启用后台任务或去掉 async 参数。"
                .to_string(),
            None,
        );
    }
    let Some(registry) = tool_jobs else {
        return (
            "错误：当前执行环境不支持后台任务（async）。请去掉 async 参数。".to_string(),
            None,
        );
    };
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        return (
            "错误：后台任务（async）暂不支持 Docker 同步工具沙盒模式；请使用宿主模式或去掉 async 参数。"
                .to_string(),
            None,
        );
    }

    let parsed = parse_run_command_json(args);
    let effective_allowed_arc =
        match resolve_run_command_shell_allowlist(cfg, effective_working_dir, web_ctx, &parsed, "run_command", true)
            .await
        {
            Ok(a) => a,
            Err(e) => return e,
        };
    let gate = match approve_external_run_command_paths_if_needed(
        cfg.as_ref(),
        args,
        effective_working_dir,
        effective_allowed_arc.as_ref(),
        web_ctx,
        "run_command",
        true,
    )
    .await
    {
        Ok(g) => g,
        Err(e) => return (e, None),
    };
    let skip_arg_safety = matches!(gate, ExternalPathGate::Approved);

    let prepared =
        match tools::prepare_run_command_for_pty_spawn(
            args,
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
    let args_value: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let timeout_secs = args_value.get("timeout_secs").and_then(|x| x.as_u64());
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
    let id = match crate::cm_internal::tool_jobs::enqueue_and_launch(
        registry,
        effective_working_dir.to_path_buf(),
        None,
        spawn,
        args.to_string(),
    ) {
        Ok(id) => id,
        Err(crate::cm_internal::tool_jobs::RegisterError::QueueFull) => {
            return (
                "错误：后台任务队列已满，请稍后重试或去掉 async 参数。".to_string(),
                None,
            );
        }
        Err(crate::cm_internal::tool_jobs::RegisterError::AtCapacity) => {
            return (
                "错误：后台任务注册表已达条目上限，请稍后重试。".to_string(),
                None,
            );
        }
    };
    let poll_url = format!("/tools/jobs/{id}");
    let output = format!("已创建后台任务 {id}，轮询 GET {poll_url}（结果可通过轮询接口获取）。");
    let inject = Some(serde_json::json!({
        "tool_job": {
            "tool_job_id": id,
            "tool_job_poll_url": poll_url,
            "tool_job_status": "queued",
        }
    }));
    (output, inject)
}
