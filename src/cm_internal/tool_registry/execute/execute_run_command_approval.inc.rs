struct ParsedRunCommandJson {
    cmd: String,
    command_raw: String,
    cmd_args: Vec<String>,
    script: String,
}

fn parse_run_command_json(args: &str) -> ParsedRunCommandJson {
    let v: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let command_raw = v
        .get("command")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let cmd = command_raw.to_lowercase();
    let cmd_args: Vec<String> = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let script = tools::join_run_command_shell_script(&command_raw, &cmd_args);
    ParsedRunCommandJson {
        cmd,
        command_raw,
        cmd_args,
        script,
    }
}

fn cmd_missing_from_allowlist(cmd: &str, allowed: &[String]) -> bool {
    !cmd.is_empty() && !allowed.iter().any(|c| c.eq_ignore_ascii_case(cmd))
}

async fn request_unknown_cmd_approval(
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
    cmd: &str,
    script: &str,
    needs_shell: bool,
    async_mode: bool,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    if async_mode {
        return Err((
            format!(
                "错误：后台任务（async）要求命令 '{}' 已在白名单或已 AllowAlways 批准；请先 AllowAlways，或去掉 async 参数。",
                cmd
            ),
            None,
        ));
    }
    let allow_handles = crate::cm_internal::tool_approval::shared_allowlist_handles_web(web_ctx);
    let cmd_show = if script.is_empty() {
        cmd.to_string()
    } else {
        script.to_string()
    };
    let spec = crate::cm_internal::tool_approval::approval_spec_run_command_unknown_cmd(cmd, &cmd_show);
    let decision_opt = if web_ctx.is_some() {
        match crate::cm_internal::tool_approval::request_tool_interactive_approval(
            web_ctx.map(crate::cm_internal::tool_approval::web_tool_runtime_approval_sink),
            &spec,
            "tool_registry::run_command approval",
        )
        .await
        {
            Ok(d) => Some(d),
            Err(crate::cm_internal::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                return Err((
                    crate::cm_internal::tool_approval::INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR
                        .to_string(),
                    None,
                ));
            }
        }
    } else {
        return Err((
            format!(
                "命令 '{}' 不在白名单中，且审批通道不可用。请在请求中提供 approval_session_id 以启用命令审批流程。",
                cmd
            ),
            None,
        ));
    };
    apply_unknown_cmd_decision(cfg, cmd, needs_shell, &cmd_show, decision_opt, &allow_handles).await
}

async fn apply_unknown_cmd_decision(
    cfg: &Arc<AgentConfig>,
    cmd: &str,
    needs_shell: bool,
    cmd_show: &str,
    decision_opt: Option<CommandApprovalDecision>,
    allow_handles: &crate::cm_internal::tool_approval::SharedAllowlistHandles<'_>,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    let Some(decision) = decision_opt else {
        return Ok(Arc::clone(&cfg.command_exec.allowed_commands));
    };
    match decision {
        CommandApprovalDecision::Deny => {
            Err((format!("用户拒绝执行命令：{}", cmd_show.trim()), None))
        }
        CommandApprovalDecision::AllowOnce => Ok(extend_allowlist_with_cmd_and_optional_bash(
            &cfg.command_exec.allowed_commands,
            cmd,
            needs_shell,
        )),
        CommandApprovalDecision::AllowAlways => {
            crate::cm_internal::tool_approval::persist_allowlist_key(allow_handles, cmd).await;
            Ok(extend_allowlist_with_cmd_and_optional_bash(
                &cfg.command_exec.allowed_commands,
                cmd,
                needs_shell,
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)] // 与 run_command_resolve_effective_allowlist 同组参数
async fn resolve_unknown_cmd_allowlist(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    cmd: &str,
    command_raw: &str,
    script: &str,
    needs_shell: bool,
    async_mode: bool,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    if crate::cm_internal::tools::run_command_invocation_targets_workspace_script_or_executable(
        effective_working_dir,
        command_raw,
    ) {
        return Ok(extend_allowed_commands_arc(
            &cfg.command_exec.allowed_commands,
            cmd,
        ));
    }
    let already_allowed = match web_ctx {
        Some(w) => w.persistent_allowlist_shared.lock().await.contains(cmd),
        None => false,
    };
    if already_allowed {
        return Ok(extend_allowed_commands_arc(
            &cfg.command_exec.allowed_commands,
            cmd,
        ));
    }
    request_unknown_cmd_approval(cfg, web_ctx, cmd, script, needs_shell, async_mode).await
}

/// 解析 `run_command` 白名单与交互审批，返回最终生效的 `allowed_commands` 快照（可能与配置不同）。
/// `async_mode=true` 时**拒绝**任何会触发交互审批的路径（后台任务无法关联单次 AllowOnce 审批）。
#[allow(clippy::too_many_arguments)] // 白名单解析：配置、目录、Web 上下文、命令/脚本与审批模式
async fn run_command_resolve_effective_allowlist(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    cmd: &str,
    command_raw: &str,
    script: &str,
    needs_shell: bool,
    async_mode: bool,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    let effective_allowed_arc = Arc::clone(&cfg.command_exec.allowed_commands);
    if !cmd_missing_from_allowlist(cmd, effective_allowed_arc.as_ref()) {
        return Ok(effective_allowed_arc);
    }
    resolve_unknown_cmd_allowlist(
        cfg,
        effective_working_dir,
        web_ctx,
        cmd,
        command_raw,
        script,
        needs_shell,
        async_mode,
    )
    .await
}

fn extend_allowlist_with_cmd_and_optional_bash(
    base: &Arc<[String]>,
    cmd: &str,
    needs_shell: bool,
) -> Arc<[String]> {
    let with_cmd = extend_allowed_commands_arc(base, cmd);
    if needs_shell {
        extend_allowed_commands_arc(&with_cmd, "bash")
    } else {
        with_cmd
    }
}

/// glob/`$VAR` 且白名单已有 bash：静默包装。Web 上独立 argv 操作符即使已有 bash 也再审（避免 `ls && rm` 绕过单命令白名单）。无审批通道且已有 bash 时仍包装。
fn posix_shell_wrap_needs_interactive_approval(
    command_raw: &str,
    cmd_args: &[String],
    bash_on_allowlist: bool,
    has_web_ctx: bool,
) -> bool {
    let expansion = tools::argv_needs_shell_expansion(command_raw, cmd_args);
    let operators = tools::argv_has_shell_operators(command_raw, cmd_args);
    if !expansion && !operators {
        return false;
    }
    if !bash_on_allowlist {
        return true;
    }
    operators && has_web_ctx
}

/// 白名单无 bash/sh 但 argv 需要 glob/`$VAR` 时，或 Web 上 argv 含 `&&`/`|` 等操作符时：审批完整脚本。
/// `async_mode=true` 时拒绝需要交互审批的 bash 包装。
async fn approve_posix_shell_wrap_if_needed(
    command_raw: &str,
    cmd_args: &[String],
    script: &str,
    effective_allowed: Arc<[String]>,
    web_ctx: Option<&WebToolRuntime>,
    sse_command: &str,
    async_mode: bool,
) -> Result<Arc<[String]>, String> {
    if !posix_shell_wrap_needs_interactive_approval(
        command_raw,
        cmd_args,
        tools::posix_shell_on_allowlist(effective_allowed.as_ref()).is_some(),
        web_ctx.is_some(),
    ) {
        return Ok(effective_allowed);
    }
    if web_ctx.is_none() {
        return Err(format!(
            "错误：{sse_command} 脚本含 glob / 变量 / 管道等，需经 bash -c 执行，但白名单无 bash/sh 且无审批通道。完整脚本：{}",
            script.trim()
        ));
    }
    if async_mode {
        return Err(format!(
            "错误：后台任务（async）不允许 bash -c 包装的交互审批；请先将 bash/sh 列入白名单，或去掉 async 参数。完整脚本：{}",
            script.trim()
        ));
    }
    let spec = tool_approval::approval_spec_shell_script(sse_command, script);
    let allow_handles = tool_approval::shared_allowlist_handles_web(web_ctx);
    match tool_approval::interactive_gate_after_whitelist_miss(
        web_ctx.map(tool_approval::web_tool_runtime_approval_sink),
        &spec,
        "tool_registry::run_command shell script approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => {
            Ok(extend_allowed_commands_arc(&effective_allowed, "bash"))
        }
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(format!("已拒绝：{msg}")),
        Err(ToolApprovalWebError::ChannelUnavailable) => {
            Err(tool_approval::INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR.to_string())
        }
    }
}

async fn resolve_run_command_shell_allowlist(
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    web_ctx: Option<&WebToolRuntime>,
    parsed: &ParsedRunCommandJson,
    sse_command: &str,
    async_mode: bool,
) -> Result<Arc<[String]>, (String, Option<serde_json::Value>)> {
    let (policy_cmd, policy_args) =
        tools::peel_cd_prefix_argv_for_shell_policy(&parsed.command_raw, &parsed.cmd_args);
    let needs_shell = tools::argv_needs_posix_shell_wrap(&policy_cmd, &policy_args);
    let allowed = run_command_resolve_effective_allowlist(
        cfg,
        effective_working_dir,
        web_ctx,
        parsed.cmd.as_str(),
        parsed.command_raw.as_str(),
        parsed.script.as_str(),
        needs_shell,
        async_mode,
    )
    .await?;
    match approve_posix_shell_wrap_if_needed(
        policy_cmd.as_str(),
        &policy_args,
        parsed.script.as_str(),
        allowed,
        web_ctx,
        sse_command,
        async_mode,
    )
    .await
    {
        Ok(a) => Ok(a),
        Err(e) => Err((e, None)),
    }
}

/// 工作区外路径 / `..` 预检三态：仅 [`ExternalPathGate::Approved`] 可置 `skip_arg_safety`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalPathGate {
    NotNeeded,
    Approved,
}

/// 配置关或无危险参数 → [`NotNeeded`]；Docker / 无通道 / 拒绝 → `Err`；批准 → [`Approved`]。
/// `async_mode=true` 时拒绝需要交互审批的工作区外路径。
async fn approve_external_run_command_paths_if_needed(
    cfg: &AgentConfig,
    args_json: &str,
    working_dir: &Path,
    allowed_commands: &[String],
    web_ctx: Option<&WebToolRuntime>,
    sse_command: &str,
    async_mode: bool,
) -> Result<ExternalPathGate, String> {
    if !cfg.command_exec.allow_external_path_with_approval {
        return Ok(ExternalPathGate::NotNeeded);
    }
    let unsafe_args = match tools::scan_run_command_unsafe_args_json(
        args_json,
        working_dir,
        allowed_commands,
    ) {
        Ok(v) => v,
        Err(e) => return Err(e.extended_user_message()),
    };
    if unsafe_args.is_empty() {
        return Ok(ExternalPathGate::NotNeeded);
    }
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        return Err(format!(
            "错误：Docker 同步工具沙盒模式下不支持工作区外路径参数（{}）。请改用宿主模式或去掉外部路径 / 路径穿越形 \"..\"。",
            unsafe_args.join(", ")
        ));
    }
    if web_ctx.is_none() {
        return Err(format!(
            "错误：{sse_command} 访问工作区外路径（{}）需要审批通道（当前无可用会话）。",
            unsafe_args.join(", ")
        ));
    }
    if async_mode {
        return Err(format!(
            "错误：后台任务（async）不支持工作区外路径（{}）的交互审批；请去掉外部路径 / 路径穿越形 \"..\"，或去掉 async 参数。",
            unsafe_args.join(", ")
        ));
    }
    let detail_paths = unsafe_args.join(", ");
    let spec =
        tool_approval::approval_spec_workspace_external_path(sse_command, &detail_paths);
    let allow_handles = tool_approval::shared_allowlist_handles_web(web_ctx);
    match tool_approval::interactive_gate_after_whitelist_miss(
        web_ctx.map(tool_approval::web_tool_runtime_approval_sink),
        &spec,
        "tool_registry::run_command external path approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => Ok(ExternalPathGate::Approved),
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(format!("已拒绝：{}", msg)),
        Err(ToolApprovalWebError::ChannelUnavailable) => {
            Err(tool_approval::INTERACTIVE_GATE_CHANNEL_UNAVAILABLE_ERR.to_string())
        }
    }
}
