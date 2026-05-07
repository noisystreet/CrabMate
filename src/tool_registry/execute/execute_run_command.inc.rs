/// `sync_default_tool_sandbox_mode = docker` 时，在宿主完成审批/白名单后把本类工具交给容器内 `tool-runner-internal`。
async fn dispatch_non_sync_tool_to_docker(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    kind: &str,
    args: &str,
    runner_cfg_path: Result<PathBuf, String>,
) -> Option<(String, Option<serde_json::Value>)> {
    if env.cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode
        != SyncDefaultToolSandboxMode::Docker
    {
        return None;
    }
    if !workspace_is_set {
        return Some((
            "错误：未设置工作区，无法在 Docker 沙盒中执行该工具（请先设置工作区目录）。"
                .to_string(),
            None,
        ));
    }
    let path = match runner_cfg_path {
        Ok(p) => p,
        Err(e) => return Some((e, None)),
    };
    let inv = crate::tool_sandbox::ToolInvocationLine {
        kind: kind.to_string(),
        tool: None,
        args_json: args.to_string(),
    };
    let out = crate::tool_sandbox::run_tool_in_docker(
        env.sandbox_backend,
        env.cfg.as_ref(),
        effective_working_dir,
        path,
        inv,
    )
    .await;
    Some(match out {
        Ok(s) => (s, None),
        Err(e) => (e, None),
    })
}

#[allow(clippy::too_many_arguments)] // Web + CLI 双路径审批共享实现
async fn execute_run_command_impl(
    env: &ToolExecEnv<'_>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    workspace_changed: &mut bool,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
    name: &str,
    args: &str,
) -> (String, Option<serde_json::Value>) {
    let cfg = env.cfg;
    if !workspace_is_set {
        return (web_tool_err_workspace_not_set("执行命令"), None);
    }
    if let Some(ctx) = cli_ctx {
        ctx.record_run_command_attempt();
    }
    let v: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let command_raw = v
        .get("command")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let cmd = command_raw.to_lowercase();
    let arg_preview = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let base_allowed = Arc::clone(&cfg.command_exec.allowed_commands);
    let mut effective_allowed_arc: Arc<[String]> = base_allowed;
    if !cmd.is_empty()
        && !effective_allowed_arc
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&cmd))
    {
        if crate::tools::run_command_invocation_targets_workspace_script_or_executable(
            effective_working_dir,
            command_raw,
        ) {
            effective_allowed_arc = extend_allowed_commands_arc(&effective_allowed_arc, &cmd);
        } else {
            let already_allowed = match (web_ctx, cli_ctx) {
                (Some(w), _) => w.persistent_allowlist_shared.lock().await.contains(&cmd),
                (None, Some(c)) => c.persistent_allowlist_shared.lock().await.contains(&cmd),
                (None, None) => false,
            };
            if already_allowed {
                effective_allowed_arc = extend_allowed_commands_arc(&effective_allowed_arc, &cmd);
            } else {
                let allow_handles = crate::tool_approval::SharedAllowlistHandles {
                    web: web_ctx.map(|w| &w.persistent_allowlist_shared),
                    cli: cli_ctx.map(|c| &c.persistent_allowlist_shared),
                };
                let cmd_show = if arg_preview.is_empty() {
                    cmd.clone()
                } else {
                    format!("{} {}", cmd, arg_preview)
                };
                let spec = crate::tool_approval::ApprovalRequestSpec {
                    capability: crate::tool_approval::SensitiveCapability::HostShell,
                    sse_command: cmd.clone(),
                    sse_args: arg_preview.clone(),
                    allowlist_key: None,
                    cli_title: "run_command 审批",
                    cli_detail: format!("命令不在白名单:\n{}", cmd_show.trim()),
                    web_timeline_prefix_zh: "命令审批：",
                };
                let decision_opt = if let Some(ctx) = cli_ctx {
                    if ctx.auto_approve_all_non_whitelist_run_command
                        || ctx
                            .extra_allowlist_commands
                            .iter()
                            .any(|e| e.eq_ignore_ascii_case(&cmd))
                    {
                        Some(CommandApprovalDecision::AllowOnce)
                    } else {
                        crate::tool_approval::request_tool_interactive_approval(
                            None,
                            Some(crate::tool_approval::CliApprovalInput {
                                auto_approve_all_sensitive: false,
                                tui_blocking_approval_tx: ctx.tui_blocking_approval_tx.clone(),
                            }),
                            &spec,
                            "tool_registry::run_command approval",
                        )
                        .await
                        .ok()
                    }
                } else if web_ctx.is_some() {
                    match crate::tool_approval::request_tool_interactive_approval(
                        web_ctx.map(|w| w.approval_sink()),
                        None,
                        &spec,
                        "tool_registry::run_command approval",
                    )
                    .await
                    {
                        Ok(d) => Some(d),
                        Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                            return ("错误：审批通道不可用，请重试。".to_string(), None);
                        }
                    }
                } else {
                    return (
                        format!(
                            "命令 '{}' 不在白名单中，且审批通道不可用。请在请求中提供 approval_session_id 以启用命令审批流程。",
                            cmd
                        ),
                        None,
                    );
                };
                if let Some(decision) = decision_opt {
                    match decision {
                        CommandApprovalDecision::Deny => {
                            if let Some(c) = cli_ctx {
                                c.record_run_command_denial();
                            }
                            return (format!("用户拒绝执行命令：{}", cmd_show.trim()), None);
                        }
                        CommandApprovalDecision::AllowOnce => {
                            effective_allowed_arc = extend_allowed_commands_arc(
                                &cfg.command_exec.allowed_commands,
                                &cmd,
                            );
                        }
                        CommandApprovalDecision::AllowAlways => {
                            crate::tool_approval::persist_allowlist_key(&allow_handles, &cmd).await;
                            effective_allowed_arc = extend_allowed_commands_arc(
                                &cfg.command_exec.allowed_commands,
                                &cmd,
                            );
                        }
                    }
                }
            }
        }
    }

    if let Some((s, inj)) = dispatch_non_sync_tool_to_docker(
        env,
        effective_working_dir,
        workspace_is_set,
        "run_command",
        args,
        crate::tool_sandbox::write_runner_config_json_with_allowed_commands(
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

    let name_in = name.to_string();
    let cmd_timeout = cfg.command_exec.command_timeout_secs;
    let cfg = Arc::clone(cfg);
    let work_dir = effective_working_dir.to_path_buf();
    let args_cloned = args.to_string();
    let handle = tokio::task::spawn_blocking(move || {
        let ctx = tools::tool_context_for(
            cfg.as_ref(),
            effective_allowed_arc.as_ref(),
            work_dir.as_path(),
        );
        tools::run_tool(&name_in, &args_cloned, &ctx)
    });
    let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
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
            error!(target: "crabmate", "命令执行超时 tool={}", name);
            format!("命令执行超时（{} 秒）", cmd_timeout)
        }
    };
    if tools::is_compile_command_success(args, &s) {
        *workspace_changed = true;
    }
    (s, None)
}

/// 并行只读批内 **`http_fetch`**：在 `spawn_blocking` 之前串行完成解析与白名单/审批，避免多请求竞态修改 `persistent_allowlist`。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或本就匹配前缀。
pub(crate) async fn prefetch_http_fetch_parallel_approvals(
    tool_calls: &[ToolCall],
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
    cli_ctx: Option<&CliToolRuntime>,
) -> HashMap<(String, String), String> {
    let mut failures: HashMap<(String, String), String> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for tc in tool_calls {
        if tc.function.name != "http_fetch" {
            continue;
        }
        let key = (tc.function.name.clone(), tc.function.arguments.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        let args = tc.function.arguments.as_str();
        let (url, method, _) = match tools::http_fetch::parse_http_fetch_args(args) {
            Ok(x) => x,
            Err(e) => {
                failures.insert(key, format!("错误：{}", e));
                continue;
            }
        };
        let storage_key = tools::http_fetch::storage_key(&url);
        let approval_args = tools::http_fetch::approval_args_display(method, &url);
        let allowed_by_cfg = tools::http_fetch::url_matches_allowed_prefixes(
            &url,
            &cfg.http_fetch.http_fetch_allowed_prefixes,
        );
        let allowed_by_list = match (web_ctx, cli_ctx) {
            (Some(w), _) => w
                .persistent_allowlist_shared
                .lock()
                .await
                .contains(&storage_key),
            (None, Some(c)) => c
                .persistent_allowlist_shared
                .lock()
                .await
                .contains(&storage_key),
            (None, None) => false,
        };
        if allowed_by_cfg || allowed_by_list {
            continue;
        }
        if web_ctx.is_none() && cli_ctx.is_none() {
            failures.insert(
                key,
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
            );
            continue;
        }
        let spec = crate::tool_approval::ApprovalRequestSpec {
            capability: crate::tool_approval::SensitiveCapability::OutboundHttpRead,
            sse_command: "http_fetch".to_string(),
            sse_args: approval_args.clone(),
            allowlist_key: Some(storage_key.clone()),
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
            "tool_registry::http_fetch approval parallel prefetch",
            &allow_handles,
        )
        .await
        {
            Ok(crate::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                failures.insert(key, msg);
            }
            Err(crate::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                failures.insert(key, "错误：审批通道不可用，请重试。".to_string());
            }
        }
    }
    failures
}
