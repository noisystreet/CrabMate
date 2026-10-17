struct ParsedRunCommandJson {
    spec: tools::CommandSpec,
    cmd: String,
    script: String,
    argv_tail: String,
}

/// 与执行侧共用 [`tools::parse_command_spec`]（含 JSON 修复、路径归一化与命令拆分）。
fn parse_run_command_json(
    args: &str,
    working_dir: &Path,
) -> Result<ParsedRunCommandJson, tools::RunCommandError> {
    let spec = tools::parse_command_spec(args, working_dir)?;
    let cmd = spec.command.to_lowercase();
    let argv_tail = tools::join_run_command_argv_tail(&spec.args);
    let script = match spec.script.as_deref() {
        Some(s) => s.to_string(),
        None => tools::join_run_command_shell_script(&spec.command, &spec.args),
    };
    Ok(ParsedRunCommandJson {
        spec,
        cmd,
        script,
        argv_tail,
    })
}

fn cmd_missing_from_allowlist(cmd: &str, allowed: &[String]) -> bool {
    !tools::allowlist_allows_all(allowed)
        && !cmd.is_empty()
        && !allowed.iter().any(|c| c.eq_ignore_ascii_case(cmd))
}

/// 执行/审批模式：由 [`evaluate_policy`] 统一消费，避免各 gate 自行判断 async / Docker。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecMode {
    Sync,
    Async,
    Docker,
    Pty,
}

/// 一次策略评估期间生效的能力快照。
#[derive(Debug, Clone, Copy)]
struct PolicyCapabilities {
    mode: ExecMode,
    has_approval_channel: bool,
}

/// 策略评估上下文。
struct PolicyContext<'a> {
    cfg: &'a Arc<AgentConfig>,
    workspace: &'a Path,
    web_ctx: Option<&'a WebToolRuntime>,
    mode: ExecMode,
    sse_command: &'a str,
}

impl PolicyContext<'_> {
    fn capabilities(&self) -> PolicyCapabilities {
        let docker = self.cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode
            == SyncDefaultToolSandboxMode::Docker;
        // 同步宿主调用在 Docker 沙盒下仍先过同一套审批，因此这里把 Sync 提升为 Docker。
        let mode = if docker && self.mode == ExecMode::Sync {
            ExecMode::Docker
        } else {
            self.mode
        };
        PolicyCapabilities {
            mode,
            has_approval_channel: self.web_ctx.is_some(),
        }
    }
}

/// 策略拒绝/不可用的归因（顺序与现网三道 gate 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyReason {
    UnknownCommand,
    PosixShellWrap,
    ExternalPath,
}

#[derive(Debug)]
enum PolicyError {
    /// 用户拒绝，或交互审批返回拒绝。
    Denied {
        reason: PolicyReason,
        message: String,
    },
    /// 缺少审批通道，或当前执行模式不支持交互审批。
    ApprovalUnavailable {
        reason: PolicyReason,
        message: String,
    },
    /// 参数扫描失败等非审批类错误。
    InvalidArgs {
        reason: PolicyReason,
        message: String,
    },
}

impl PolicyError {
    fn reason(&self) -> PolicyReason {
        match self {
            Self::Denied { reason, .. }
            | Self::ApprovalUnavailable { reason, .. }
            | Self::InvalidArgs { reason, .. } => *reason,
        }
    }

    /// 转成工具调用返回的 `(正文, 注入)`；审批错误不注入结构化数据。
    fn into_message(self) -> (String, Option<serde_json::Value>) {
        let reason = self.reason();
        let message = match self {
            Self::Denied { message, .. }
            | Self::ApprovalUnavailable { message, .. }
            | Self::InvalidArgs { message, .. } => message,
        };
        log::debug!("run_command policy rejected: reason={reason:?}");
        (message, None)
    }
}

/// 已发生的审批（`AllowAlways` 的持久化副作用仍留在本层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyApproval {
    UnknownCommandAllowOnce,
    UnknownCommandAllowAlways,
    PosixShellWrap,
    ExternalPath,
}

/// 一次策略评估的最终生效参数。
#[derive(Debug)]
struct PolicyDecision {
    allowed_commands: Arc<[String]>,
    skip_arg_safety: bool,
    capabilities: PolicyCapabilities,
    approvals: Vec<PolicyApproval>,
}

/// 收敛三道审批 gate：未知命令 → shell 包装 → 工作区外路径。
///
/// 顺序、弹窗次数与错误文案与拆分前逐字一致；`async` / Docker / 通道可用性统一由
/// [`PolicyCapabilities`] 判定。
async fn evaluate_policy(
    parsed: &ParsedRunCommandJson,
    ctx: &PolicyContext<'_>,
) -> Result<PolicyDecision, PolicyError> {
    let capabilities = ctx.capabilities();
    let (policy_cmd, policy_args) =
        tools::peel_cd_prefix_argv_for_shell_policy(&parsed.spec.command, &parsed.spec.args);
    let needs_shell = parsed.spec.script.is_some()
        || tools::argv_needs_posix_shell_wrap(&policy_cmd, &policy_args);

    let mut approvals = Vec::new();
    let (allowed_commands, approval) = resolve_unknown_cmd_allowlist(parsed, needs_shell, ctx).await?;
    approvals.extend(approval);
    let (allowed_commands, approval) = approve_posix_shell_wrap_if_needed(
        parsed,
        policy_cmd.as_str(),
        &policy_args,
        allowed_commands,
        ctx,
    )
    .await?;
    approvals.extend(approval);
    let gate = approve_external_run_command_paths_if_needed(parsed, &allowed_commands, ctx).await?;
    let skip_arg_safety = matches!(gate, ExternalPathGate::Approved);
    if skip_arg_safety {
        approvals.push(PolicyApproval::ExternalPath);
    }

    let decision = PolicyDecision {
        allowed_commands,
        skip_arg_safety,
        capabilities,
        approvals,
    };
    log::debug!(
        "run_command policy: mode={:?} skip_arg_safety={} approvals={:?}",
        decision.capabilities.mode,
        decision.skip_arg_safety,
        decision.approvals
    );
    Ok(decision)
}

async fn request_unknown_cmd_approval(
    ctx: &PolicyContext<'_>,
    cmd: &str,
    script: &str,
    argv_tail: &str,
    needs_shell: bool,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    if ctx.capabilities().mode == ExecMode::Async {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::UnknownCommand,
            message: format!(
                "错误：后台任务（async）要求命令 '{}' 已在白名单或已 AllowAlways 批准；请先 AllowAlways，或去掉 async 参数。",
                cmd
            ),
        });
    }
    let Some(web_ctx) = ctx.web_ctx else {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::UnknownCommand,
            message: format!(
                "命令 '{}' 不在白名单中，且审批通道不可用。请在请求中提供 approval_session_id 以启用命令审批流程。",
                cmd
            ),
        });
    };
    let allow_handles = crate::cm_internal::tool_approval::shared_allowlist_handles_web(ctx.web_ctx);
    let cmd_show = if script.is_empty() {
        cmd.to_string()
    } else {
        script.to_string()
    };
    let spec = crate::cm_internal::tool_approval::approval_spec_run_command_unknown_cmd(
        cmd,
        argv_tail,
        &cmd_show,
    );
    let decision = match crate::cm_internal::tool_approval::request_tool_interactive_approval(
        Some(crate::cm_internal::tool_approval::web_tool_runtime_approval_sink(web_ctx)),
        &spec,
        "tool_registry::run_command approval",
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            return Err(PolicyError::ApprovalUnavailable {
                reason: PolicyReason::UnknownCommand,
                message: e.user_message_zh().to_string(),
            });
        }
    };
    apply_unknown_cmd_decision(
        ctx.cfg,
        cmd,
        needs_shell,
        &cmd_show,
        decision,
        &allow_handles,
    )
    .await
}

async fn apply_unknown_cmd_decision(
    cfg: &Arc<AgentConfig>,
    cmd: &str,
    needs_shell: bool,
    cmd_show: &str,
    decision: CommandApprovalDecision,
    allow_handles: &crate::cm_internal::tool_approval::SharedAllowlistHandles<'_>,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    let extended = |approval| {
        (
            extend_allowlist_with_cmd_and_optional_bash(
                &cfg.command_exec.allowed_commands,
                cmd,
                needs_shell,
            ),
            Some(approval),
        )
    };
    match decision {
        CommandApprovalDecision::Deny => Err(PolicyError::Denied {
            reason: PolicyReason::UnknownCommand,
            message: format!("用户拒绝执行命令：{}", cmd_show.trim()),
        }),
        CommandApprovalDecision::AllowOnce => Ok(extended(PolicyApproval::UnknownCommandAllowOnce)),
        CommandApprovalDecision::AllowAlways => {
            crate::cm_internal::tool_approval::persist_allowlist_key(allow_handles, cmd).await;
            Ok(extended(PolicyApproval::UnknownCommandAllowAlways))
        }
    }
}

async fn resolve_unknown_cmd_allowlist(
    parsed: &ParsedRunCommandJson,
    needs_shell: bool,
    ctx: &PolicyContext<'_>,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    let base = Arc::clone(&ctx.cfg.command_exec.allowed_commands);
    if !cmd_missing_from_allowlist(&parsed.cmd, base.as_ref()) {
        return Ok((base, None));
    }
    if crate::cm_internal::tools::run_command_invocation_targets_workspace_script_or_executable(
        ctx.workspace,
        &parsed.spec.command,
    ) {
        return Ok((extend_allowed_commands_arc(&base, &parsed.cmd), None));
    }
    if let Some(w) = ctx.web_ctx
        && w.persistent_allowlist_shared
            .lock()
            .await
            .contains(parsed.cmd.as_str())
    {
        return Ok((extend_allowed_commands_arc(&base, &parsed.cmd), None));
    }
    request_unknown_cmd_approval(
        ctx,
        parsed.cmd.as_str(),
        parsed.script.as_str(),
        parsed.argv_tail.as_str(),
        needs_shell,
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
/// `async` 模式拒绝需要交互审批的 bash 包装。
async fn approve_posix_shell_wrap_if_needed(
    parsed: &ParsedRunCommandJson,
    policy_cmd: &str,
    policy_args: &[String],
    effective_allowed: Arc<[String]>,
    ctx: &PolicyContext<'_>,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    if tools::allowlist_allows_all(effective_allowed.as_ref()) {
        return Ok((effective_allowed, None));
    }
    let bash_on_allowlist = tools::posix_shell_on_allowlist(effective_allowed.as_ref()).is_some();
    // 显式 `script` 一律交给 shell 执行：白名单无 bash/sh 时必须走交互审批。
    let needs_approval = if parsed.spec.script.is_some() {
        !bash_on_allowlist
    } else {
        posix_shell_wrap_needs_interactive_approval(
            policy_cmd,
            policy_args,
            bash_on_allowlist,
            ctx.web_ctx.is_some(),
        )
    };
    if !needs_approval {
        return Ok((effective_allowed, None));
    }
    let capabilities = ctx.capabilities();
    if !capabilities.has_approval_channel {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::PosixShellWrap,
            message: format!(
                "错误：{} 脚本含 glob / 变量 / 管道等，需经 bash -c 执行，但白名单无 bash/sh 且无审批通道。完整脚本：{}",
                ctx.sse_command,
                parsed.script.trim()
            ),
        });
    }
    if capabilities.mode == ExecMode::Async {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::PosixShellWrap,
            message: format!(
                "错误：后台任务（async）不允许 bash -c 包装的交互审批；请先将 bash/sh 列入白名单，或去掉 async 参数。完整脚本：{}",
                parsed.script.trim()
            ),
        });
    }
    let spec = tool_approval::approval_spec_shell_script(ctx.sse_command, parsed.script.as_str());
    let allow_handles = tool_approval::shared_allowlist_handles_web(ctx.web_ctx);
    match tool_approval::interactive_gate_after_whitelist_miss(
        ctx.web_ctx.map(tool_approval::web_tool_runtime_approval_sink),
        &spec,
        "tool_registry::run_command shell script approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => Ok((
            extend_allowed_commands_arc(&effective_allowed, "bash"),
            Some(PolicyApproval::PosixShellWrap),
        )),
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(PolicyError::Denied {
            reason: PolicyReason::PosixShellWrap,
            message: format!("已拒绝：{msg}"),
        }),
        Err(e) => Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::PosixShellWrap,
            message: e.user_message_zh().to_string(),
        }),
    }
}

/// 工作区外路径 / `..` 预检三态：仅 [`ExternalPathGate::Approved`] 可置 `skip_arg_safety`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalPathGate {
    NotNeeded,
    Approved,
}

/// 配置关或无危险参数 → [`NotNeeded`]；Docker / 无通道 / 拒绝 → `Err`；批准 → [`Approved`]。
/// `async` 模式拒绝需要交互审批的工作区外路径。
async fn approve_external_run_command_paths_if_needed(
    parsed: &ParsedRunCommandJson,
    allowed_commands: &[String],
    ctx: &PolicyContext<'_>,
) -> Result<ExternalPathGate, PolicyError> {
    if !ctx.cfg.command_exec.allow_external_path_with_approval {
        return Ok(ExternalPathGate::NotNeeded);
    }
    let unsafe_args = match tools::scan_run_command_unsafe_args_spec(
        &parsed.spec,
        ctx.workspace,
        allowed_commands,
    ) {
        Ok(v) => v,
        Err(e) => {
            return Err(PolicyError::InvalidArgs {
                reason: PolicyReason::ExternalPath,
                message: e.extended_user_message(),
            });
        }
    };
    if unsafe_args.is_empty() {
        return Ok(ExternalPathGate::NotNeeded);
    }
    let capabilities = ctx.capabilities();
    if capabilities.mode == ExecMode::Docker {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::ExternalPath,
            message: format!(
                "错误：Docker 同步工具沙盒模式下不支持工作区外路径参数（{}）。请改用宿主模式或去掉外部路径 / 路径穿越形 \"..\"。",
                unsafe_args.join(", ")
            ),
        });
    }
    if !capabilities.has_approval_channel {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::ExternalPath,
            message: format!(
                "错误：{} 访问工作区外路径（{}）需要审批通道（当前无可用会话）。",
                ctx.sse_command,
                unsafe_args.join(", ")
            ),
        });
    }
    if capabilities.mode == ExecMode::Async {
        return Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::ExternalPath,
            message: format!(
                "错误：后台任务（async）不支持工作区外路径（{}）的交互审批；请去掉外部路径 / 路径穿越形 \"..\"，或去掉 async 参数。",
                unsafe_args.join(", ")
            ),
        });
    }
    let detail_paths = unsafe_args.join(", ");
    let spec =
        tool_approval::approval_spec_workspace_external_path(ctx.sse_command, &detail_paths);
    let allow_handles = tool_approval::shared_allowlist_handles_web(ctx.web_ctx);
    match tool_approval::interactive_gate_after_whitelist_miss(
        ctx.web_ctx.map(tool_approval::web_tool_runtime_approval_sink),
        &spec,
        "tool_registry::run_command external path approval",
        &allow_handles,
    )
    .await
    {
        Ok(InteractiveGateOutcome::Allowed) => Ok(ExternalPathGate::Approved),
        Ok(InteractiveGateOutcome::Denied(msg)) => Err(PolicyError::Denied {
            reason: PolicyReason::ExternalPath,
            message: format!("已拒绝：{}", msg),
        }),
        Err(e) => Err(PolicyError::ApprovalUnavailable {
            reason: PolicyReason::ExternalPath,
            message: e.user_message_zh().to_string(),
        }),
    }
}
