struct RunCommandSyncHostInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    args: &'a str,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    timeout_secs: Option<u64>,
    parsed: ParsedRunCommandJson,
    decision: PolicyDecision,
    capabilities: RunCommandCapabilities,
}

/// 同步宿主执行：解析与审批已由 [`execute_run_command_impl`] 完成，此处只做 Docker 转交与宿主 spawn。
async fn execute_run_command_sync_host(
    invoke: RunCommandSyncHostInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandSyncHostInvoke {
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
    } = invoke;
    let cfg = env.cfg;
    let effective_allowed_arc = decision.allowed_commands;
    let skip_arg_safety = decision.skip_arg_safety;

    if capabilities.docker_sandbox
        && let Some((s, inj)) = dispatch_non_sync_tool_to_docker(
            env,
            effective_working_dir,
            workspace_is_set,
            "run_command",
            args,
            crate::cm_internal::tool_sandbox::write_runner_config_json_with_allowed_commands(
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

    let s = spawn_run_command_host(SpawnRunCommandHost {
        cfg: cfg.as_ref(),
        spec: &parsed.spec,
        work_dir: effective_working_dir.to_path_buf(),
        allowed: Arc::clone(&effective_allowed_arc),
        skip_arg_safety,
        timeout_secs,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
    })
    .await;
    if tools::is_compile_command_success(args, &s) {
        *workspace_changed = true;
    }
    (s, None)
}

struct SpawnRunCommandHost<'a> {
    cfg: &'a AgentConfig,
    spec: &'a tools::CommandSpec,
    work_dir: PathBuf,
    allowed: Arc<[String]>,
    skip_arg_safety: bool,
    timeout_secs: Option<u64>,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

async fn spawn_run_command_host(p: SpawnRunCommandHost<'_>) -> String {
    let SpawnRunCommandHost {
        cfg,
        spec,
        work_dir,
        allowed,
        skip_arg_safety,
        timeout_secs,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
    } = p;
    let cmd_timeout = timeout_secs
        .map(|t| t.clamp(1, 600))
        .unwrap_or(cfg.command_exec.command_timeout_secs);
    let max_out = cfg.command_exec.command_max_output_len;
    let test_cache_enabled = cfg.chat_queues_cache.test_result_cache_enabled;
    let test_cache_max = cfg.chat_queues_cache.test_result_cache_max_entries;
    let github_token = crate::cm_tools::github_token::resolve_token_plaintext();
    let cancel = cancel.clone();
    let sse_closed = sse_out_tx.cloned();
    let extra_stop = sse_closed.map(|tx| {
        std::sync::Arc::new(move || tx.is_closed()) as std::sync::Arc<dyn Fn() -> bool + Send + Sync>
    });
    let wait = crate::cm_tools::subprocess_session::SubprocessWaitCtl {
        wall: Some(std::time::Duration::from_secs(cmd_timeout.max(1))),
        cancel,
        extra_stop,
        chunk_sink: run_command_chunk_sink(
            tool_call_id.to_string(),
            sse_out_tx.cloned(),
            sse_control_mirror.cloned(),
        ),
        uncapped_live: false,
    };
    let fut = async move {
        let test_cache = test_cache_enabled.then_some(tools::RunCommandTestCacheOpts {
            enabled: true,
            max_entries: test_cache_max,
            workspace_root: work_dir.as_path(),
        });
        match tools::run_async(
            spec,
            tools::RunOptions {
                max_output_len: max_out,
                allowed_commands: allowed.as_ref(),
                working_dir: work_dir.as_path(),
                test_cache,
                skip_arg_safety,
                wait: &wait,
            },
        )
        .await
        {
            Ok(outcome) => {
                let rendered = outcome.render(max_out);
                match outcome.session_stop_tool_error(&rendered) {
                    Some(e) => e.message,
                    None => rendered,
                }
            }
            Err(e) => e.extended_user_message(),
        }
    };
    crate::cm_tools::github_token::with_request_github_token(github_token, fut).await
}
