async fn execute_http_fetch_impl(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
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
    let spec = crate::cm_internal::tool_approval::approval_spec_http_fetch(&approval_args, &key);
    if let Err(out) = http_tool_preflight(HttpToolPreflightParams {
        env,
        effective_working_dir,
        workspace_is_set,
        web_ctx,
        args,
        tool_name: "http_fetch",
        url: &url,
        storage_key: &key,
        sse_log_label: "tool_registry::http_fetch approval",
        approval_spec: &spec,
    })
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch.http_fetch_max_response_bytes;
    let url_owned = url.clone();
    let user_agent = cfg.http_fetch.http_fetch_user_agent.clone();
    let outer_wall = http_fetch_outer_wall_secs(cfg);
    let s = spawn_blocking_http_tool("http_fetch", name, outer_wall, move || {
        tools::http_fetch::fetch_with_method(
            &url_owned,
            method,
            text_format,
            &user_agent,
            timeout_secs,
            max_body,
        )
    })
    .await;
    (s, None)
}

async fn execute_http_request_impl(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    web_ctx: Option<&WebToolRuntime>,
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
    let approval_args =
        tools::http_fetch::approval_args_display_request(method, &url, has_body);
    let spec =
        crate::cm_internal::tool_approval::approval_spec_http_request(&approval_args, &key);
    if let Err(out) = http_tool_preflight(HttpToolPreflightParams {
        env,
        effective_working_dir,
        workspace_is_set,
        web_ctx,
        args,
        tool_name: "http_request",
        url: &url,
        storage_key: &key,
        sse_log_label: "tool_registry::http_request approval",
        approval_spec: &spec,
    })
    .await
    {
        return out;
    }
    let timeout_secs = cfg.http_fetch.http_fetch_timeout_secs.max(1);
    let max_body = cfg.http_fetch.http_fetch_max_response_bytes;
    let url_fetch = url.clone();
    let user_agent = cfg.http_fetch.http_fetch_user_agent.clone();
    let outer_wall = http_request_outer_wall_secs(cfg);
    let s = spawn_blocking_http_tool("http_request", name, outer_wall, move || {
        tools::http_fetch::request_with_json_body(
            &url_fetch,
            method,
            json_body.as_ref(),
            text_format,
            &user_agent,
            timeout_secs,
            max_body,
        )
    })
    .await;
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
        crate::cm_internal::tool_sandbox::write_runner_config_json(cfg.as_ref()),
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
        crate::cm_internal::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return out;
    }
    let name_in = name.to_string();
    let search_timeout = cfg.web_search.web_search_timeout_secs;
    // 外圈略长于内层（worbrow/reqwest）超时，给浏览器收尾留宽限，避免外圈先砍掉等待后残留进程。
    let outer_wall = web_search_outer_wall_secs(cfg.as_ref());
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
    let s = match tokio::time::timeout(Duration::from_secs(outer_wall), handle).await {
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
            error!(
                target: "crabmate",
                "联网搜索超时 tool={} configured_secs={} outer_wall_secs={}",
                name,
                search_timeout,
                outer_wall
            );
            format!("联网搜索超时（{} 秒）", search_timeout)
        }
    };
    (s, None)
}
