async fn try_dispatch_dynamic_tool(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    if !crate::cm_internal::dynamic_tools::is_dynamic_tool_name(name) {
        return None;
    }
    if !workspace_is_set {
        return Some((
            web_tool_err_workspace_not_set("执行动态工具").to_string(),
            None,
        ));
    }
    let def = match crate::cm_internal::dynamic_tools::resolve_runtime_def(effective_working_dir, name) {
        Ok(Some(d)) => d,
        Ok(None) => return Some((format!("未知工具：{}", name), None)),
        Err(e) => return Some((format!("错误：动态工具加载失败：{}", e), None)),
    };
    let args_owned = args.to_string();
    let wd = effective_working_dir.to_path_buf();
    let cfg2 = Arc::clone(cfg);
    let wall_secs = cfg.command_exec.command_timeout_secs.max(1);
    let handle = tokio::task::spawn_blocking(move || {
        crate::cm_internal::dynamic_tools::run_dynamic_tool(
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
    mcp_turn: Option<&crate::cm_internal::mcp::McpTurnHandle>,
    name: &str,
    args: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    if !crate::cm_internal::mcp::is_mcp_proxy_tool(name) {
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
    let mcp_args = crate::cm_internal::tool_call_explain::strip_explain_why_if_present(args);
    let out = crate::cm_internal::mcp::call_mcp_tool(
        &guard,
        remote.as_str(),
        mcp_args.as_str(),
        Duration::from_secs(turn.tool_timeout_secs.max(1)),
        cfg.command_exec.command_max_output_len,
    )
    .await;
    Some((out, None))
}

async fn dispatch_sync_default_tool(
    p: SyncDefaultToolDispatchArgs<'_>,
) -> (String, Option<serde_json::Value>) {
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
    } = p;
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        if !workspace_is_set {
            return (
                "错误：未设置工作区，无法在 Docker 沙盒中执行 SyncDefault 工具（请先设置工作区目录）。"
                    .to_string(),
                None,
            );
        }
        let out = crate::cm_internal::tool_sandbox::run_sync_default_in_docker(
            env.sandbox_backend,
            cfg.as_ref(),
            effective_working_dir,
            name,
            args,
        )
        .await;
        return match out {
            Ok(s) => (s, None),
            Err(e) => (e, None),
        };
    }

    // `read_dir` 外部路径审批：绝对路径或含 `..` 时需用户确认（不走白名单）。
    if name == "read_dir" {
        let web_ctx = http_tool_approval_context(runtime);
        if let Err(msg) = approve_external_read_dir_if_needed(args, web_ctx).await {
            return (msg, None);
        }
    }

    if sync_default_runs_inline(cfg.as_ref(), name) {
        let hosts = crate::cm_internal::memory_tool_hosts::DispatchMemoryHosts::from_dispatch_inputs(
            cfg.as_ref(),
            long_term_memory.clone(),
            long_term_memory_scope_id.as_deref(),
        );
        let ctx = tools::tool_context_for_with_read_cache_and_memory(
            cfg.as_ref(),
            cfg.command_exec.allowed_commands.as_ref(),
            effective_working_dir,
            read_file_turn_cache.as_ref().map(|a| a.as_ref()),
            workspace_changelist.as_ref(),
            Some(hosts.codebase_ref()),
            hosts.long_term_ref(),
        );
        return (tools::run_tool(name, args, &ctx), None);
    }
    let cfg2 = Arc::clone(cfg);
    let tool_name = tc.function.name.clone();
    let tool_args = tc.function.arguments.clone();
    let work_dir = effective_working_dir.to_path_buf();
    let rfc = read_file_turn_cache.clone();
    let wcl = workspace_changelist.clone();
    let ltm2 = long_term_memory.clone();
    let ltm_scope2 = long_term_memory_scope_id.clone();
    let wall_secs = parallel_tool_wall_timeout_secs(cfg.as_ref(), name);
    let github_token = crate::cm_tools::github_token::resolve_token_plaintext();
    let handle = tokio::task::spawn_blocking(move || {
        crate::cm_tools::github_token::with_request_github_token_blocking(github_token, || {
            let hosts = crate::cm_internal::memory_tool_hosts::DispatchMemoryHosts::from_dispatch_inputs(
                cfg2.as_ref(),
                ltm2,
                ltm_scope2.as_deref(),
            );
            let ctx = tools::tool_context_for_with_read_cache_and_memory(
                cfg2.as_ref(),
                cfg2.command_exec.allowed_commands.as_ref(),
                work_dir.as_path(),
                rfc.as_ref().map(|a| a.as_ref()),
                wcl.as_ref(),
                Some(hosts.codebase_ref()),
                hosts.long_term_ref(),
            );
            tools::run_tool(&tool_name, &tool_args, &ctx)
        })
    });
    let result = match tokio::time::timeout(Duration::from_secs(wall_secs), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "工具执行异常 tool={} error={:?}",
                name,
                e
            );
            format!("工具执行异常：{:?}", e)
        }
        Err(_) => {
            error!(target: "crabmate", "工具执行超时 tool={} wall_secs={}", name, wall_secs);
            format!("工具执行超时（{} 秒）", wall_secs)
        }
    };
    (result, None)
}

/// 工具分发入口：默认单次执行；开启 `[tool_registry] tool_retry_*` 且资格门通过时，
/// 对瞬时类失败（`error_code ∈ tool_retry_error_codes`）做**透明重试**——失败中间态不写
/// 模型上下文，仅最终结果返回；重试间隔按 `backoff_for_retry` 指数退避，用户取消立即停止。
pub async fn dispatch_tool(mut p: DispatchToolParams<'_>) -> (String, Option<serde_json::Value>) {
    let spec = ToolRetrySpec::from_config(p.policy.cfg.as_ref());
    if !spec.tool_retry_eligible(p.policy.cfg.as_ref(), p.call.name, p.call.args) {
        return dispatch_tool_inner(&mut p).await;
    }
    let max_attempts = spec.max_attempts;
    let mut attempt: u64 = 1;
    loop {
        let (out, payload) = dispatch_tool_inner(&mut p).await;
        let error_code =
            crate::cm_tools::tool_result::parse_legacy_output(p.call.name, &out).error_code;
        if !spec.should_retry(error_code.as_deref(), attempt) {
            return (out, payload);
        }
        // 用户取消：停止重试，返回最后一次结果。
        if p
            .obs
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        {
            return (out, payload);
        }
        log::info!(
            target: "crabmate",
            "tool_retry tool={} attempt={}/{} error_code={} args_preview={}",
            p.call.name,
            attempt,
            max_attempts,
            error_code.as_deref().unwrap_or("-"),
            crate::cm_tools::redact::tool_arguments_preview_for_log(p.call.args)
        );
        let backoff = spec.backoff_for_retry(attempt);
        if backoff > 0 {
            let cancel = p.obs.cancel.clone();
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(backoff)) => {}
                _ = wait_for_tool_cancel(cancel.as_ref()) => {
                    // 退避期间用户取消：立即停止，返回最后一次结果。
                    return (out, payload);
                }
            }
        }
        attempt += 1;
    }
}

/// 等待用户取消标志置位（供重试退避期间响应取消）；`None` 表示无取消源，永久等待。
async fn wait_for_tool_cancel(
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) {
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

/// 单次工具执行（含审批、沙盒与 handler 分发）；供 [`dispatch_tool`] 重试循环按可变借用复用。
async fn dispatch_tool_inner(p: &mut DispatchToolParams<'_>) -> (String, Option<serde_json::Value>) {
    let DispatchToolParams {
        runtime,
        call,
        workspace,
        policy,
        obs,
        memory,
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
    };
    if !crate::cm_internal::agent_role_turn::tool_allowed_for_turn(name, turn_allow) {
        return (crate::cm_internal::agent_role_turn::turn_tool_denied_message(name), None);
    }

    if let Some(out) =
        try_dispatch_dynamic_tool(cfg, effective_working_dir, workspace_is_set, name, args).await
    {
        return out;
    }

    if let Some(out) = try_dispatch_mcp_proxy_tool(cfg, mcp_turn, name, args).await {
        return out;
    }

    let args_processed =
        match crate::cm_internal::tool_call_explain::require_explain_for_mutation(cfg.as_ref(), name, args) {
            Ok(c) => c,
            Err(e) => return (e, None),
        };
    let args = args_processed.as_ref();

    let hid = handler_lookup.id_for(name);

    // Web 未设置工作区时：仍允许出网类工具；禁止所有 SyncDefault（否则 `working_dir` 会回落到配置目录，等效于未选工作区仍可读本地树）。
    if !workspace_is_set && matches!(hid, HandlerId::SyncDefault) {
        return (
            web_tool_err_workspace_not_set("执行内置工具").to_string(),
            None,
        );
    }

    match hid {
        HandlerId::Workflow => {
            // `workflow_execute` 由 `agent::workflow_tool_dispatch` 调度（避免本模块依赖 `PerCoordinator` / `workflow`）。
            (
                "内部错误：workflow_execute 须由 agent 层调度，不应进入 dispatch_tool".to_string(),
                None,
            )
        }
        HandlerId::RunCommand => {
            let ToolRuntime {
                workspace_changed,
                ctx,
            } = runtime;
            execute_run_command_impl(RunCommandHostInvoke {
                env: &env,
                effective_working_dir,
                workspace_is_set,
                workspace_changed,
                web_ctx: ctx,
                name,
                args,
                tool_call_id: tc.id.as_str(),
                sse_out_tx,
                sse_control_mirror,
                cancel,
                tool_jobs,
            })
            .await
        }
        HandlerId::TerminalSession => {
            let ToolRuntime {
                workspace_changed,
                ctx,
            } = runtime;
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
            })
            .await
        }
        HandlerId::GetWeather => {
            execute_get_weather_web(&env, effective_working_dir, workspace_is_set, name, args).await
        }
        HandlerId::WebSearch => {
            execute_web_search_web(&env, effective_working_dir, workspace_is_set, name, args).await
        }
        HandlerId::HttpFetch => {
            let web_ctx = http_tool_approval_context(runtime);
            execute_http_fetch_impl(
                &env,
                effective_working_dir,
                workspace_is_set,
                web_ctx,
                name,
                args,
            )
            .await
        }
        HandlerId::HttpRequest => {
            let web_ctx = http_tool_approval_context(runtime);
            execute_http_request_impl(
                &env,
                effective_working_dir,
                workspace_is_set,
                web_ctx,
                name,
                args,
            )
            .await
        }
        HandlerId::SyncDefault => {
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
            })
            .await
        }
    }
}

/// 并行只读批内 `SyncDefault` 预审批：当前仅覆盖 `read_dir` 的工作区外路径访问。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或无需审批。
pub async fn prefetch_parallel_syncdefault_approvals(
    tool_calls: &[ToolCall],
    web_ctx: Option<&WebToolRuntime>,
    handler_lookup: &HandlerLookupTable,
) -> HashMap<(String, String), String> {
    let mut failures: HashMap<(String, String), String> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for tc in tool_calls {
        if handler_lookup.id_for(tc.function.name.as_str()) != HandlerId::SyncDefault {
            continue;
        }
        if tc.function.name != "read_dir" {
            continue;
        }
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Err(msg) =
            approve_external_read_dir_if_needed(tc.function.arguments.as_str(), web_ctx).await
        {
            failures.insert(key, msg);
        }
    }
    failures
}
