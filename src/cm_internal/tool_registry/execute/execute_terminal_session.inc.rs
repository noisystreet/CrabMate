struct TerminalSessionExecInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    tool_call_id: &'a str,
    sse_encoder: Option<&'a dyn crate::cm_sse_protocol::sse::SseEncoder>,
}

fn terminal_session_precheck(
    workspace_is_set: bool,
    docker_sandbox: bool,
) -> Option<(String, Option<serde_json::Value>)> {
    if !workspace_is_set {
        return Some((
            format!(
                "错误：未设置工作区，禁止使用交互式终端。{}",
                WEB_WORKSPACE_PANEL_HINT
            ),
            None,
        ));
    }
    if docker_sandbox {
        return Some((
            "错误：terminal_session 在 Docker 同步工具沙盒模式下不可用。".to_string(),
            None,
        ));
    }
    None
}

fn parse_terminal_session_args(args: &str) -> Result<serde_json::Value, (String, Option<serde_json::Value>)> {
    serde_json::from_str(args).map_err(|e| (format!("错误：参数 JSON 无效: {e}"), None))
}

fn terminal_session_exec_new_session(v: &serde_json::Value, action_raw: &str) -> bool {
    action_raw == "exec"
        && !v
            .get("session_id")
            .and_then(|x| x.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
}

async fn execute_terminal_session_impl(
    invoke: TerminalSessionExecInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let TerminalSessionExecInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        args,
        sse_out_tx,
        sse_control_mirror,
        tool_call_id,
        sse_encoder,
    } = invoke;
    let cfg = env.cfg;
    if let Some(err) = terminal_session_precheck(
        workspace_is_set,
        cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker,
    ) {
        return err;
    }

    let v = match parse_terminal_session_args(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let action_raw = v
        .get("action")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let exec_new_session = terminal_session_exec_new_session(&v, &action_raw);
    let exec_rc_json = exec_new_session.then(|| {
        serde_json::json!({
            "command": v.get("command"),
            "args": v.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
        })
        .to_string()
    });

    let effective_allowed = match terminal_session_effective_allowed(
        cfg,
        effective_working_dir,
        web_ctx,
        exec_rc_json.as_deref(),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => return e,
    };

    let skip_arg_safety = match terminal_session_skip_arg_safety(
        cfg.as_ref(),
        effective_working_dir,
        web_ctx,
        exec_rc_json.as_deref(),
        effective_allowed.as_ref(),
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return (e, None),
    };

    let wall_secs = parallel_tool_wall_timeout_secs(cfg.as_ref(), "terminal_session");
    let fut = crate::cm_internal::terminal_session::execute_terminal_session(
        cfg,
        effective_working_dir,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        effective_allowed.as_ref(),
        sse_encoder,
        skip_arg_safety,
    );

    let result = match tokio::time::timeout(Duration::from_secs(wall_secs), fut).await {
        Ok(s) => s,
        Err(_) => format!("terminal_session 执行超时（{} 秒）", wall_secs),
    };

    if let Some(rc_line) = exec_rc_json.as_ref()
        && tools::is_compile_command_success(rc_line.as_str(), &result)
    {
        *workspace_changed = true;
    }

    (result, None)
}

async fn terminal_session_effective_allowed(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    exec_rc_json: Option<&str>,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    let Some(rc) = exec_rc_json else {
        return Ok(Arc::clone(&cfg.command_exec.allowed_commands));
    };
    let parsed = parse_run_command_json(rc);
    resolve_run_command_shell_allowlist(
        cfg,
        effective_working_dir,
        web_ctx,
        &parsed,
        "terminal_session",
        false,
    )
    .await
}

async fn terminal_session_skip_arg_safety(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    exec_rc_json: Option<&str>,
    effective_allowed: &[String],
) -> Result<bool, String> {
    let Some(rc) = exec_rc_json else {
        return Ok(false);
    };
    match approve_external_run_command_paths_if_needed(
        cfg,
        rc,
        effective_working_dir,
        effective_allowed,
        web_ctx,
        "terminal_session",
        false,
    )
    .await
    {
        Ok(ExternalPathGate::Approved) => Ok(true),
        Ok(ExternalPathGate::NotNeeded) => Ok(false),
        Err(e) => Err(e),
    }
}
