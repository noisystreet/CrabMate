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

/// 本次策略评估的**命令能力快照**：生效白名单的唯一持有者。
///
/// R3（设计 §17.3）：[`PolicyDecision`] 只**借用**本结构读结果，不再自带
/// `Arc<[String]>` 副本。评估过程在基快照上就地扩展（`AllowOnce` / `AllowAlways` /
/// 工作区脚本 + `bash`），扩展结果写回同一快照，语义与拆分前逐字一致。
///
/// S3 引入 `PermissionCaps` 后并入其 `command_scope` 轴
/// （`Any` ↔ `*` 哨兵、`Allowlist` ↔ 具体列表）。
///
/// **不要 `Clone`、不要跨评估复用**：评估以它持有的列表为基做扩展，
/// 复用会让上一轮 `AllowOnce` 的临时批准成为下一轮的基白名单。
#[derive(Debug)]
struct RunCommandCaps {
    /// 生效白名单；含 `*` 哨兵时等价全放行（见 [`tools::allowlist_allows_all`]）。
    allowed_commands: Arc<[String]>,
}

impl RunCommandCaps {
    /// 以 `cfg.command_exec.allowed_commands` 为基快照。
    fn from_cfg(cfg: &AgentConfig) -> Self {
        Self {
            allowed_commands: Arc::clone(&cfg.command_exec.allowed_commands),
        }
    }

    /// 以权限等级投影出的 `command_scope` 为基快照（S3）。
    ///
    /// - `None`（L0 / L1）→ **空**白名单；调用方已在 early-deny 拒掉，这里是双保险，
    ///   空表让「未知命令」走拒绝而不是审批。
    /// - `Any`（L4）→ `*` 哨兵，等价于 [`tools::allowlist_allows_all`]。
    /// - `Allowlist`（L2 / L3）→ 该列表（L2 与 cfg 同源，L3 略窄）。
    /// - `permission = None`（未接线路径）→ 回落 `cfg`，行为与 S3 前一致。
    fn from_permission(
        cfg: &AgentConfig,
        permission: Option<&crate::cm_agent::permission::PermissionCaps>,
    ) -> Self {
        match permission {
            None => Self::from_cfg(cfg),
            Some(caps) => Self {
                allowed_commands: caps
                    .command_allowlist()
                    .unwrap_or_else(|| Arc::from(Vec::<String>::new())),
            },
        }
    }
}

/// 策略评估上下文。
struct PolicyContext<'a> {
    cfg: &'a Arc<AgentConfig>,
    workspace: &'a Path,
    web_ctx: Option<&'a WebToolRuntime>,
    mode: ExecMode,
    sse_command: &'a str,
    /// 本回合权限能力快照；`None` = 未接线，回落 `cfg` 白名单（S3）。
    permission: Option<&'a crate::cm_agent::permission::PermissionCaps>,
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

    /// 以当前配置为基构造命令能力快照（每次评估一份，评估后就地扩展）。
    fn command_caps(&self) -> RunCommandCaps {
        RunCommandCaps::from_permission(self.cfg, self.permission)
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

/// 一次策略评估的最终生效结果。
///
/// `caps` 是白名单的**单一真源**（借用，不由本结构持有）；`skip_arg_safety` 是它的
/// 派生值——仅工作区外路径审批通过时为 `true`。`ExecMode` 留在
/// [`PolicyCapabilities`]：那是**执行形态**，不是权限面（设计 §7.4）。
#[derive(Debug)]
struct PolicyDecision<'a> {
    caps: &'a RunCommandCaps,
    skip_arg_safety: bool,
    capabilities: PolicyCapabilities,
    approvals: Vec<PolicyApproval>,
}

/// 收敛三道审批 gate：未知命令 → shell 包装 → 工作区外路径。
///
/// 顺序、弹窗次数与错误文案与拆分前逐字一致；`async` / Docker / 通道可用性统一由
/// [`PolicyCapabilities`] 判定。
///
/// `caps` **必须每次评估新建**（用 [`PolicyContext::command_caps`]）：它既是基白名单的
/// 唯一来源，也是扩展结果的落点。复用上一轮的快照会让 `AllowOnce` 的临时批准变成下一轮的
/// 基白名单——等于静默放宽命令面。
async fn evaluate_policy<'a>(
    parsed: &ParsedRunCommandJson,
    ctx: &PolicyContext<'_>,
    caps: &'a mut RunCommandCaps,
) -> Result<PolicyDecision<'a>, PolicyError> {
    let capabilities = ctx.capabilities();
    // L0 / L1（`command_scope = None`）：命令类工具已在 early-deny 与可见性处被拒，这里再拦一次。
    // 空白名单本可让「未知命令」直接进审批，`AllowOnce` 会把它放行——那就等于用一次弹窗
    // 绕过等级边界。双保险的落点放在审批之前。
    if let Some(caps) = ctx.permission
        && matches!(
            caps.command_scope,
            crate::cm_agent::permission::CommandScope::None
        )
    {
        return Err(PolicyError::Denied {
            reason: PolicyReason::UnknownCommand,
            message: crate::cm_agent::permission::permission_denied_message(caps, ctx.sse_command),
        });
    }
    let (policy_cmd, policy_args) =
        tools::peel_cd_prefix_argv_for_shell_policy(&parsed.spec.command, &parsed.spec.args);
    let needs_shell = parsed.spec.script.is_some()
        || tools::argv_needs_posix_shell_wrap(&policy_cmd, &policy_args);

    let mut approvals = Vec::new();
    let (allowed_commands, approval) =
        resolve_unknown_cmd_allowlist(parsed, needs_shell, ctx, &caps.allowed_commands).await?;
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

    // 生效白名单落回快照：`caps` 是唯一持有者，`PolicyDecision` 只借用它。
    caps.allowed_commands = allowed_commands;
    let decision = PolicyDecision {
        caps: &*caps,
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

/// 未知命令的交互审批；`base` 是快照里的白名单（批准后基于**它**扩展，不是 `cfg`）。
async fn request_unknown_cmd_approval(
    ctx: &PolicyContext<'_>,
    cmd: &str,
    script: &str,
    argv_tail: &str,
    needs_shell: bool,
    base: &Arc<[String]>,
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
    let allow_handles = crate::cm_internal::tool_approval::shared_allowlist_handles_web(
        ctx.web_ctx,
        ctx.permission.map(|c| c.level),
    );
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
        base,
        cmd,
        needs_shell,
        &cmd_show,
        decision,
        &allow_handles,
    )
    .await
}

/// 把一次性审批结论落到「快照基白名单 + 新命令（+ `bash`）」上。
///
/// `base` 必须来自 [`RunCommandCaps`] 而非 `cfg`：等级上限收窄后两者不再相等，
/// 用 `cfg` 作基会把配置全量白名单带进生效集，绕过等级边界（设计 §6 只收紧不放宽）。
async fn apply_unknown_cmd_decision(
    base: &Arc<[String]>,
    cmd: &str,
    needs_shell: bool,
    cmd_show: &str,
    decision: CommandApprovalDecision,
    allow_handles: &crate::cm_internal::tool_approval::SharedAllowlistHandles<'_>,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    let extended = |approval| {
        (
            extend_allowlist_with_cmd_and_optional_bash(base, cmd, needs_shell),
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

/// 以快照里的基白名单做未知命令判定；`base` 由 [`RunCommandCaps`] 持有（单一真源）。
async fn resolve_unknown_cmd_allowlist(
    parsed: &ParsedRunCommandJson,
    needs_shell: bool,
    ctx: &PolicyContext<'_>,
    base: &Arc<[String]>,
) -> Result<(Arc<[String]>, Option<PolicyApproval>), PolicyError> {
    if !cmd_missing_from_allowlist(&parsed.cmd, base.as_ref()) {
        return Ok((Arc::clone(base), None));
    }
    if crate::cm_internal::tools::run_command_invocation_targets_workspace_script_or_executable(
        ctx.workspace,
        &parsed.spec.command,
    ) {
        return Ok((extend_allowed_commands_arc(base, &parsed.cmd), None));
    }
    // persistent allowlist 的键带**等级作用域前缀**（设计 §6.3）：L3 批过的命令在 L2
    // 不得继续命中——否则这一行会抢在下面的 L2 硬拒之前把命令放行。
    if let Some(w) = ctx.web_ctx {
        let stored_cmd_key = crate::cm_types::scoped_allowlist_key(
            ctx.permission.map(|c| c.level),
            parsed.cmd.as_str(),
        );
        if w.persistent_allowlist_shared
            .lock()
            .await
            .contains(&stored_cmd_key)
        {
            return Ok((extend_allowed_commands_arc(base, &parsed.cmd), None));
        }
    }
    // L2（`approval = Deny`）：未知命令直接拒，**不**弹审批（无人值守档的硬边界）。
    // 顺序放在「工作区内脚本 / persistent allowlist 放行」之后：那两条不是越界，
    // 与等级无关的既有放行道保持不变。
    if let Some(caps) = ctx.permission
        && caps.approval == crate::cm_agent::permission::ApprovalPolicy::Deny
    {
        return Err(PolicyError::Denied {
            reason: PolicyReason::UnknownCommand,
            message: format!(
                "错误：{} 不在当前权限等级（{}）的命令白名单内：{}。",
                parsed.cmd,
                caps.level.as_str(),
                caps.level.promise_zh()
            ),
        });
    }
    request_unknown_cmd_approval(
        ctx,
        parsed.cmd.as_str(),
        parsed.script.as_str(),
        parsed.argv_tail.as_str(),
        needs_shell,
        base,
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
    // L2（`approval = Deny`）：需要 shell 包装的场景同样硬拒，不弹审批。
    if let Some(caps) = ctx.permission
        && caps.approval == crate::cm_agent::permission::ApprovalPolicy::Deny
    {
        return Err(PolicyError::Denied {
            reason: PolicyReason::PosixShellWrap,
            message: format!(
                "错误：{} 需要 POSIX shell 包装，而当前权限等级（{}）不允许交互审批：{}。完整脚本：{}",
                ctx.sse_command,
                caps.level.as_str(),
                caps.level.promise_zh(),
                parsed.script
            ),
        });
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
    let allow_handles = tool_approval::shared_allowlist_handles_web(
        ctx.web_ctx,
        ctx.permission.map(|c| c.level),
    );
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

/// 权限等级对「工作区外路径参数」的**直接结论**（S3）：
/// `None` = 继续走现状（配置开关 + 交互审批），`Some(Ok/Err)` = 不弹窗直接定论。
///
/// - L4（`path_scope = Any`）：路径面已放宽，直接 `Approved`（等价 `skip_arg_safety`）。
/// - L2（`approval = Deny`）：硬拒，不弹审批。
fn external_path_gate_by_caps(
    ctx: &PolicyContext<'_>,
    unsafe_args: &[String],
) -> Option<Result<ExternalPathGate, PolicyError>> {
    let caps = ctx.permission?;
    if caps.path_scope.is_unrestricted() {
        return Some(Ok(ExternalPathGate::Approved));
    }
    if caps.approval == crate::cm_agent::permission::ApprovalPolicy::Deny {
        return Some(Err(PolicyError::Denied {
            reason: PolicyReason::ExternalPath,
            message: format!(
                "错误：当前权限等级（{}）下不允许工作区外路径参数（{}）：{}。",
                caps.level.as_str(),
                unsafe_args.join(", "),
                caps.level.promise_zh()
            ),
        }));
    }
    None
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
    if let Some(gate) = external_path_gate_by_caps(ctx, &unsafe_args) {
        return gate;
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
    let allow_handles = tool_approval::shared_allowlist_handles_web(
        ctx.web_ctx,
        ctx.permission.map(|c| c.level),
    );
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
