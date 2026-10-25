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
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
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

/// exec 新会话：把 `terminal_session` 的 `command` / `args` 投影成 `run_command` 载荷并解析一次。
fn parse_terminal_session_exec(
    v: &serde_json::Value,
    action_raw: &str,
    effective_working_dir: &Path,
) -> Result<Option<ParsedRunCommandJson>, (String, Option<serde_json::Value>)> {
    if !terminal_session_exec_new_session(v, action_raw) {
        return Ok(None);
    }
    let rc_json = serde_json::json!({
        "command": v.get("command"),
        "args": v.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
    })
    .to_string();
    parse_run_command_json(&rc_json, effective_working_dir)
        .map(Some)
        .map_err(|e| (e.extended_user_message(), None))
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
        cancel,
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

    let exec_parsed = match parse_terminal_session_exec(&v, &action_raw, effective_working_dir) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let (effective_allowed, skip_arg_safety) = match exec_parsed.as_ref() {
        None => (Arc::clone(&cfg.command_exec.allowed_commands), false),
        Some(parsed) => {
            let policy_ctx = PolicyContext {
                cfg,
                workspace: effective_working_dir,
                web_ctx,
                mode: ExecMode::Pty,
                sse_command: "terminal_session",
            };
            let mut policy_caps = policy_ctx.command_caps();
            match evaluate_policy(parsed, &policy_ctx, &mut policy_caps).await {
                Ok(d) => (Arc::clone(&d.caps.allowed_commands), d.skip_arg_safety),
                Err(e) => return e.into_message(),
            }
        }
    };

    let wall_secs = terminal_session_outer_wall_secs(cfg.as_ref());
    let sse = crate::cm_internal::terminal_session::TerminalSseSink {
        out_tx: sse_out_tx,
        control_mirror: sse_control_mirror,
        encoder: sse_encoder,
    };
    let fut = crate::cm_internal::terminal_session::execute_terminal_session(
        cfg,
        effective_working_dir,
        args,
        tool_call_id,
        sse,
        effective_allowed.as_ref(),
        skip_arg_safety,
        cancel,
    );

    let result = match tokio::time::timeout(Duration::from_secs(wall_secs), fut).await {
        Ok(s) => s,
        Err(_) => format!(
            "terminal_session 执行超时（{} 秒）；已按超时清理会话。",
            wall_secs
        ),
    };

    if let Some(parsed) = exec_parsed.as_ref()
        && tools::is_compile_command_success_spec(&parsed.spec, &result)
    {
        *workspace_changed = true;
    }

    (result, None)
}
