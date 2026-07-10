struct TerminalSessionExecInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    web_ctx: Option<&'a WebToolRuntime>,
    cli_ctx: Option<&'a CliToolRuntime>,
    args: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::sse::SseControlMirror>,
    tool_call_id: &'a str,
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
        cli_ctx,
        args,
        sse_out_tx,
        sse_control_mirror,
        tool_call_id,
    } = invoke;
    let cfg = env.cfg;
    if !workspace_is_set {
        return (
            format!(
                "错误：未设置工作区，禁止使用交互式终端。{}",
                WEB_WORKSPACE_PANEL_HINT
            ),
            None,
        );
    }
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        return (
            "错误：terminal_session 在 Docker 同步工具沙盒模式下不可用。".to_string(),
            None,
        );
    }

    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(e) => return (format!("错误：参数 JSON 无效: {e}"), None),
    };
    let action_raw = v
        .get("action")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    let effective_allowed: Arc<[String]> = if action_raw == "exec" {
        let sid_nonempty = v
            .get("session_id")
            .and_then(|x| x.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !sid_nonempty {
            if let Some(ctx) = cli_ctx {
                ctx.record_run_command_attempt();
            }
            let rc = serde_json::json!({
                "command": v.get("command"),
                "args": v.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
            })
            .to_string();
            let (cmd, command_raw, arg_preview) = parse_run_command_json(&rc);
            match run_command_resolve_effective_allowlist(
                cfg,
                effective_working_dir,
                web_ctx,
                cli_ctx,
                cmd.as_str(),
                command_raw.as_str(),
                arg_preview.as_str(),
            )
            .await
            {
                Ok(a) => a,
                Err(e) => return e,
            }
        } else {
            Arc::clone(&cfg.command_exec.allowed_commands)
        }
    } else {
        Arc::clone(&cfg.command_exec.allowed_commands)
    };

    let wall_secs = parallel_tool_wall_timeout_secs(cfg.as_ref(), "terminal_session");
    let fut = crate::terminal_session::execute_terminal_session(
        cfg,
        effective_working_dir,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        effective_allowed.as_ref(),
    );

    let result =
        match tokio::time::timeout(Duration::from_secs(wall_secs), fut).await {
            Ok(s) => s,
            Err(_) => format!("terminal_session 执行超时（{} 秒）", wall_secs),
        };

    if action_raw == "exec" {
        let sid_nonempty = v
            .get("session_id")
            .and_then(|x| x.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !sid_nonempty {
            let rc_line = serde_json::json!({
                "command": v.get("command"),
                "args": v.get("args").cloned().unwrap_or_else(|| serde_json::json!([])),
            })
            .to_string();
            if tools::is_compile_command_success(rc_line.as_str(), &result) {
                *workspace_changed = true;
            }
        }
    }

    (result, None)
}
