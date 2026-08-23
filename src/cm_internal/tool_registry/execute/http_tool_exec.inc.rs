// `http_fetch` / `http_request` 共用：前缀白名单、Web 审批与 Docker 分流。

/// URL 是否已由配置前缀或 persistent allowlist 放行。
async fn http_tool_url_allowed(
    cfg: &crate::cm_config::AgentConfig,
    web_ctx: Option<&WebToolRuntime>,
    url: &reqwest::Url,
    storage_key: &str,
) -> bool {
    let allowed_by_cfg = tools::http_fetch::url_matches_allowed_prefixes(
        url,
        &cfg.http_fetch.http_fetch_allowed_prefixes,
    );
    if allowed_by_cfg {
        return true;
    }
    match web_ctx {
        Some(w) => w.persistent_allowlist_shared.lock().await.contains(storage_key),
        None => false,
    }
}

/// 未放行时发起 Web 审批；`Ok(())` 表示可继续本地执行。
async fn http_tool_gate_url_approval(
    web_ctx: Option<&WebToolRuntime>,
    spec: &crate::cm_internal::tool_approval::ApprovalRequestSpec,
    sse_log_label: &'static str,
) -> Result<(), (String, Option<serde_json::Value>)> {
    if web_ctx.is_none() {
        return Err((
            tool_approval::HTTP_TOOL_NO_APPROVAL_CHANNEL_ERR.to_string(),
            None,
        ));
    }
    match tool_approval::interactive_gate_web_runtime(web_ctx, spec, sse_log_label).await {
        Ok(outcome) => tool_approval::interactive_gate_outcome_to_tool_err(outcome)
            .map_err(|msg| (msg, None)),
        Err(ToolApprovalWebError::ChannelUnavailable) => Err((
            tool_approval::INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR.to_string(),
            None,
        )),
    }
}

/// 前缀白名单 + 可选 Web 审批 + Docker 分流；通过后返回 `Ok(())` 以继续 `spawn_blocking`。
struct HttpToolPreflightParams<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    tool_name: &'static str,
    url: &'a reqwest::Url,
    storage_key: &'a str,
    sse_log_label: &'static str,
    approval_spec: &'a crate::cm_internal::tool_approval::ApprovalRequestSpec,
}

async fn http_tool_preflight(
    p: HttpToolPreflightParams<'_>,
) -> Result<(), (String, Option<serde_json::Value>)> {
    let cfg = p.env.cfg;
    if !http_tool_url_allowed(cfg.as_ref(), p.web_ctx, p.url, p.storage_key).await {
        http_tool_gate_url_approval(p.web_ctx, p.approval_spec, p.sse_log_label).await?;
    }
    if let Some(out) = dispatch_non_sync_tool_to_docker(
        p.env,
        p.effective_working_dir,
        p.workspace_is_set,
        p.tool_name,
        p.args,
        crate::cm_internal::tool_sandbox::write_runner_config_json(cfg.as_ref()),
    )
    .await
    {
        return Err(out);
    }
    Ok(())
}

async fn spawn_blocking_http_tool(
    tool_label: &'static str,
    name: &str,
    outer_wall_secs: u64,
    f: impl FnOnce() -> String + Send + 'static,
) -> String {
    let name_in = name.to_string();
    let handle = tokio::task::spawn_blocking(f);
    match tokio::time::timeout(Duration::from_secs(outer_wall_secs), handle).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            error!(
                target: "crabmate",
                "{tool_label} 任务异常 tool={} error={:?}",
                name_in,
                e
            );
            format!("{tool_label} 执行异常：{:?}", e)
        }
        Err(_) => format!("{tool_label} 超时（{} 秒）", outer_wall_secs),
    }
}

/// 并行预取：`http_fetch` URL 审批（与单工具路径共用 spec 构建器）。
async fn http_fetch_prefetch_approval(
    web_ctx: Option<&WebToolRuntime>,
    cfg: &crate::cm_config::AgentConfig,
    url: &reqwest::Url,
    storage_key: &str,
    approval_args: &str,
) -> Result<(), String> {
    if http_tool_url_allowed(cfg, web_ctx, url, storage_key).await {
        return Ok(());
    }
    let spec = tool_approval::approval_spec_http_fetch(approval_args, storage_key);
    match http_tool_gate_url_approval(
        web_ctx,
        &spec,
        "tool_registry::http_fetch approval parallel prefetch",
    )
    .await
    {
        Ok(()) => Ok(()),
        Err((msg, _)) => Err(msg),
    }
}
