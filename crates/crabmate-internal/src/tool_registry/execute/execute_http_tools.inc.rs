async fn execute_http_fetch_impl(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let cfg = env.cfg;
    let (url, method, text_format) = match tools::http_fetch::parse_http_fetch_args(args) {
        Ok(x) => x,
        Err(e) => return (format!("错误：{}", e), None),
    };
    let key = tools::http_fetch::storage_key(&url);
    let approval_args = tools::http_fetch::approval_args_display(method, &url);
    let allowed_by_cfg = tools::http_fetch::url_matches_allowed_prefixes(
        &url,
        &cfg.http_fetch.http_fetch_allowed_prefixes,
    );
    let allowed_by_list = match (web_ctx, cli_ctx) {
        (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&key),
        (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&key),
        (None, None) => false,
    };
    if !(allowed_by_cfg || allowed_by_list) {
        if web_ctx.is_none() && cli_ctx.is_none() {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpRead,
            sse_command: "http_fetch".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(key.clone()),
            cli_title: "http_fetch 审批",
            cli_detail: format!(
                "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                approval_args
            ),
            web_timeline_prefix_zh: "http_fetch 审批：",
        };
        let allow_handles = crate::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
            cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
        };
        match crate::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(|w| w.approval_sink()),
            cli_ctx.map(|c| crate::tool_approval::CliApprovalInput {
                auto_approve_all_sensitive: c.auto_approve_all_non_whitelist_run_command,
                tui_blocking_approval_tx: c.tui_blocking_approval_tx.clone(),
            }),
            &spec,
            "tool_registry::http_fetch approval",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                return (msg, None);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                return ("错误：审批通道不可用，请重试。".to_string(), None);
            }
        }
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        env,
        effective_working_dir,
        workspace_is_set,
        "http_fetch",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch.http_fetch_max_response_bytes;
    let name_in = name.to_string();
    let url_owned = url.clone();
    let outer_wall = http_fetch_outer_wall_secs(cfg);
    let handle = tokio::task::spawn_blocking(move || {
        tools::http_fetch::fetch_with_method(
            &url_owned,
            method,
            text_format,
            timeout_secs,
            max_body,
        )
    });
    let s = match tokio::time::timeout(Duration::from_secs(outer_wall), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_fetch 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("http_fetch 执行异常：{:?}", e)
        }
        Err(_) => format!("http_fetch 超时（{} 秒）", outer_wall),
    };
    (s, None)
}

async fn execute_http_request_impl(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let cfg = env.cfg;
    let (url, method, json_body, text_format) =
        match tools::http_fetch::parse_http_request_args(args) {
            Ok(x) => x,
            Err(e) => return (format!("错误：{}", e), None),
        };
    let has_body = json_body.is_some();
    let key = tools::http_fetch::request_storage_key(method, &url);
    let approval_args = tools::http_fetch::approval_args_display_request(method, &url, has_body);
    let allowed_by_cfg = tools::http_fetch::url_matches_allowed_prefixes(
        &url,
        &cfg.http_fetch.http_fetch_allowed_prefixes,
    );
    let allowed_by_list = match (web_ctx, cli_ctx) {
        (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&key),
        (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&key),
        (None, None) => false,
    };
    if !(allowed_by_cfg || allowed_by_list) {
        if web_ctx.is_none() && cli_ctx.is_none() {
            return (
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
                None,
            );
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpWrite,
            sse_command: "http_request".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(key.clone()),
            cli_title: "http_request 审批",
            cli_detail: format!(
                "URL 未匹配 http_fetch_allowed_prefixes（同源 + 路径前缀边界）：\n{}",
                approval_args
            ),
            web_timeline_prefix_zh: "http_request 审批：",
        };
        let allow_handles = crate::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
            cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
        };
        match crate::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(|w| w.approval_sink()),
            cli_ctx.map(|c| crate::tool_approval::CliApprovalInput {
                auto_approve_all_sensitive: c.auto_approve_all_non_whitelist_run_command,
                tui_blocking_approval_tx: c.tui_blocking_approval_tx.clone(),
            }),
            &spec,
            "tool_registry::http_request approval",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                return (msg, None);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                return ("错误：审批通道不可用，请重试。".to_string(), None);
            }
        }
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        env,
        effective_working_dir,
        workspace_is_set,
        "http_request",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch.http_fetch_max_response_bytes;
    let name_in = name.to_string();
    let url_fetch = url.clone();
    let outer_wall = http_request_outer_wall_secs(cfg);
    let handle = tokio::task::spawn_blocking(move || {
        tools::http_fetch::request_with_json_body(
            &url_fetch,
            method,
            json_body.as_ref(),
            text_format,
            timeout_secs,
            max_body,
        )
    });
    let s = match tokio::time::timeout(Duration::from_secs(outer_wall), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "http_request 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("http_request 执行异常：{:?}", e)
        }
        Err(_) => format!("http_request 超时（{} 秒）", outer_wall),
    };
    (s, None)
}

async fn execute_get_weather_web(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let cfg = env.cfg;
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        env,
        effective_working_dir,
        workspace_is_set,
        "get_weather",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let weather_timeout = cfg.weather_tool.weather_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.command_exec.allowed_commands.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(weather_timeout), handle).await {
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
            error!(target: "crabmate", "天气请求超时 tool={}", name);
            format!("天气请求超时（{} 秒）", weather_timeout)
        }
    };
    (s, None)
}

async fn execute_web_search_web(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let cfg = env.cfg;
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        env,
        effective_working_dir,
        workspace_is_set,
        "web_search",
        args,
        crate::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let search_timeout = cfg.web_search.web_search_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_owned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            cfg.command_exec.allowed_commands.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_owned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(search_timeout), handle).await {
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
            error!(target: "crabmate", "联网搜索超时 tool={}", name);
            format!("联网搜索超时（{} 秒）", search_timeout)
        }
    };
    (s, None)
}
