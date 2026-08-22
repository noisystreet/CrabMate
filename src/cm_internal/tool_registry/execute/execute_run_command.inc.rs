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
    let inv = crate::cm_internal::tool_sandbox::ToolInvocationLine {
        kind: kind.to_string(),
        tool: None,
        args_json: args.to_string(),
    };
    let out = crate::cm_internal::tool_sandbox::run_tool_in_docker(
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
    let allow_handles = crate::cm_internal::tool_approval::SharedAllowlistHandles {
        web: web_ctx.map(|w| &w.persistent_allowlist_shared),
    };
    let cmd_show = if script.is_empty() {
        cmd.to_string()
    } else {
        script.to_string()
    };
    let spec = crate::cm_internal::tool_approval::ApprovalRequestSpec {
        capability: crate::cm_internal::tool_approval::SensitiveCapability::HostShell,
        sse_command: cmd.to_string(),
        sse_args: cmd_show.clone(),
        allowlist_key: None,
        cli_title: "run_command 审批",
        cli_detail: format!("命令不在白名单；审批对象为完整脚本:\n{}", cmd_show.trim()),
        web_timeline_prefix_zh: "命令审批：",
    };
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
                return Err(("错误：审批通道不可用，请重试。".to_string(), None));
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
    let spec = ApprovalRequestSpec {
        capability: SensitiveCapability::HostShell,
        sse_command: sse_command.to_string(),
        sse_args: script.to_string(),
        allowlist_key: None,
        cli_title: if sse_command == "terminal_session" {
            "terminal_session 脚本审批"
        } else {
            "run_command 脚本审批"
        },
        cli_detail: format!(
            "将经 bash -c 执行整行（glob / $VAR 会展开；独立 argv 中的 && / | 等会绕过单命令白名单）：\n{}",
            script.trim()
        ),
        web_timeline_prefix_zh: "脚本审批：",
    };
    let allow_handles = SharedAllowlistHandles {
        web: web_ctx.map(|w| &w.persistent_allowlist_shared),
    };
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
            Err("错误：审批通道不可用，请重试。".to_string())
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
    let spec = ApprovalRequestSpec {
        capability: SensitiveCapability::WorkspaceExternalPath,
        sse_command: sse_command.to_string(),
        sse_args: format!("external_paths={detail_paths}"),
        allowlist_key: None,
        cli_title: if sse_command == "terminal_session" {
            "terminal_session 工作区外路径审批"
        } else {
            "run_command 工作区外路径审批"
        },
        cli_detail: format!(
            "{sse_command} 请求使用工作区外路径或 \"..\"：{detail_paths}\n仅在可信环境下批准。\n（不审计 bash/sh -c 脚本字符串内部路径。）"
        ),
        web_timeline_prefix_zh: "工作区外路径审批：",
    };
    let allow_handles = SharedAllowlistHandles {
        web: web_ctx.map(|w| &w.persistent_allowlist_shared),
    };
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
            Err("错误：审批通道不可用，请重试。".to_string())
        }
    }
}

struct RunCommandHostInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    web_ctx: Option<&'a WebToolRuntime>,
    name: &'a str,
    args: &'a str,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

fn run_command_chunk_sink(
    tool_call_id: String,
    sse_out_tx: Option<tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<crate::cm_sse_protocol::sse::SseControlMirror>,
) -> Option<crate::cm_tools::subprocess_session::SessionChunkSink> {
    if sse_out_tx.is_none() && sse_control_mirror.is_none() {
        return None;
    }
    let seq = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let utf8_out = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let utf8_err = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    Some(std::sync::Arc::new(move |stream, bytes| {
        emit_run_command_tool_output_chunk(
            seq.as_ref(),
            &tool_call_id,
            stream,
            bytes,
            sse_out_tx.as_ref(),
            sse_control_mirror.as_ref(),
            match stream {
                crate::cm_tools::subprocess_session::SessionStream::Stdout => utf8_out.as_ref(),
                crate::cm_tools::subprocess_session::SessionStream::Stderr => utf8_err.as_ref(),
            },
        )
    }))
}

fn emit_run_command_tool_output_chunk(
    seq: &std::sync::atomic::AtomicU64,
    tool_call_id: &str,
    stream: crate::cm_tools::subprocess_session::SessionStream,
    bytes: &[u8],
    sse_out_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::cm_sse_protocol::sse::SseControlMirror>,
    utf8_pending: &std::sync::Mutex<Vec<u8>>,
) -> bool {
    let finish = bytes.is_empty();
    let (text, saved) = {
        let Ok(mut pending) = utf8_pending.lock() else {
            return true;
        };
        let saved = pending.clone();
        let text = crate::cm_tools::subprocess_session::take_utf8_text(&mut pending, bytes, finish);
        (text, saved)
    };
    if text.is_empty() {
        return true;
    }
    let n = seq.load(std::sync::atomic::Ordering::SeqCst) + 1;
    let payload = crate::cm_sse_protocol::sse::protocol::SsePayload::ToolOutputChunk {
        tool_output_chunk: crate::cm_sse_protocol::sse::protocol::ToolOutputChunkBody {
            tool_call_id: tool_call_id.to_string(),
            name: Some("run_command".to_string()),
            seq: n,
            chunk: text,
            stream: Some(stream.as_sse_label().to_string()),
        },
    };
    let encoder = crate::cm_sse_protocol::sse::V2Encoder;
    let ok = crate::cm_sse_protocol::sse::send_sse_control_payload_try_send(
        sse_out_tx,
        sse_control_mirror,
        payload,
        "run_command::output_chunk",
        &encoder,
    );
    if ok {
        seq.store(n, std::sync::atomic::Ordering::SeqCst);
        true
    } else if let Ok(mut pending) = utf8_pending.lock() {
        *pending = saved;
        false
    } else {
        false
    }
}

struct RunCommandAsyncInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    web_ctx: Option<&'a WebToolRuntime>,
    args: &'a str,
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

/// `run_command` 的 `async=true` 后台路径（契约 §1.2 / §2）。
///
/// 发起时刻完成白名单 / 路径 / 审批（`async_mode=true` 拒绝一切交互审批），
/// 构造进程组可执行参数后登记后台任务并立即返回启动帧；轮询/取消走 `GET /tools/jobs/{id}` 等端点。
async fn execute_run_command_async(
    invoke: RunCommandAsyncInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandAsyncInvoke {
        env,
        effective_working_dir,
        web_ctx,
        args,
        tool_jobs,
    } = invoke;
    let cfg = env.cfg;
    if !cfg.tool_registry_policy.tool_registry_background_jobs_enabled {
        return (
            "错误：后台任务（async）未启用（[tool_registry] background_jobs_enabled=false）。请启用后台任务或去掉 async 参数。"
                .to_string(),
            None,
        );
    }
    let Some(registry) = tool_jobs else {
        return (
            "错误：当前执行环境不支持后台任务（async）。请去掉 async 参数。".to_string(),
            None,
        );
    };
    if cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker {
        return (
            "错误：后台任务（async）暂不支持 Docker 同步工具沙盒模式；请使用宿主模式或去掉 async 参数。"
                .to_string(),
            None,
        );
    }

    let parsed = parse_run_command_json(args);
    let effective_allowed_arc =
        match resolve_run_command_shell_allowlist(cfg, effective_working_dir, web_ctx, &parsed, "run_command", true)
            .await
        {
            Ok(a) => a,
            Err(e) => return e,
        };
    let gate = match approve_external_run_command_paths_if_needed(
        cfg.as_ref(),
        args,
        effective_working_dir,
        effective_allowed_arc.as_ref(),
        web_ctx,
        "run_command",
        true,
    )
    .await
    {
        Ok(g) => g,
        Err(e) => return (e, None),
    };
    let skip_arg_safety = matches!(gate, ExternalPathGate::Approved);

    let prepared =
        match tools::prepare_run_command_for_pty_spawn(
            args,
            effective_working_dir,
            effective_allowed_arc.as_ref(),
            skip_arg_safety,
        ) {
            Ok(p) => p,
            Err(e) => return (e.extended_user_message(), None),
        };
    let program = match prepared.exec_path {
        Some(p) => p.to_string_lossy().into_owned(),
        None => prepared.cmd_name,
    };
    // gh token：发起时刻解析并固化到 spawn（worker 线程无请求作用域）。
    let extra_env = if crate::cm_tools::github_token::command_basename_is_gh(&program) {
        crate::cm_tools::github_token::resolve_token_for_child_env()
            .map(|t| vec![("GH_TOKEN".to_string(), t)])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let args_value: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let timeout_secs = args_value.get("timeout_secs").and_then(|x| x.as_u64());
    let wall = Duration::from_secs(
        timeout_secs
            .map(|t| t.clamp(1, 600))
            .unwrap_or(cfg.command_exec.command_timeout_secs)
            .max(1),
    );
    let spawn = crate::cm_internal::tool_jobs::JobSpawn {
        program,
        args: prepared.cmd_args,
        cwd: prepared.effective_working_dir,
        extra_env,
        wall,
        max_output_len: cfg.command_exec.command_max_output_len,
    };
    let id = match crate::cm_internal::tool_jobs::enqueue_and_launch(
        registry,
        effective_working_dir.to_path_buf(),
        None,
        spawn,
        args.to_string(),
    ) {
        Ok(id) => id,
        Err(crate::cm_internal::tool_jobs::RegisterError::QueueFull) => {
            return (
                "错误：后台任务队列已满，请稍后重试或去掉 async 参数。".to_string(),
                None,
            );
        }
        Err(crate::cm_internal::tool_jobs::RegisterError::AtCapacity) => {
            return (
                "错误：后台任务注册表已达条目上限，请稍后重试。".to_string(),
                None,
            );
        }
    };
    let poll_url = format!("/tools/jobs/{id}");
    let output = format!("已创建后台任务 {id}，轮询 GET {poll_url}（结果可通过轮询接口获取）。");
    let inject = Some(serde_json::json!({
        "tool_job": {
            "tool_job_id": id,
            "tool_job_poll_url": poll_url,
            "tool_job_status": "queued",
        }
    }));
    (output, inject)
}

async fn execute_run_command_impl(
    invoke: RunCommandHostInvoke<'_>,
) -> (String, Option<serde_json::Value>) {
    let RunCommandHostInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        name,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
        tool_jobs,
    } = invoke;
    if !workspace_is_set {
        return (web_tool_err_workspace_not_set("执行命令"), None);
    }
    let args_value: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
    let async_flag = args_value
        .get("async")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let timeout_secs = args_value.get("timeout_secs").and_then(|x| x.as_u64());
    if async_flag {
        return execute_run_command_async(RunCommandAsyncInvoke {
            env,
            effective_working_dir,
            web_ctx,
            args,
            tool_jobs: tool_jobs.clone(),
        })
        .await;
    }
    execute_run_command_sync_host(RunCommandSyncHostInvoke {
        env,
        effective_working_dir,
        workspace_is_set,
        workspace_changed,
        web_ctx,
        name,
        args,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        cancel,
        timeout_secs,
    })
    .await
}

/// 并行只读批内 **`http_fetch`**：在 `spawn_blocking` 之前串行完成解析与白名单/审批，避免多请求竞态修改 `persistent_allowlist`。
/// 返回 `(name, args) -> 错误文案`；未出现的键表示已获准或本就匹配前缀。
pub async fn prefetch_http_fetch_parallel_approvals(
    tool_calls: &[ToolCall],
    cfg: &Arc<AgentConfig>,
    web_ctx: Option<&WebToolRuntime>,
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
        let allowed_by_list = match web_ctx {
            Some(w) => w
                .persistent_allowlist_shared
                .lock()
                .await
                .contains(&storage_key),
            None => false,
        };
        if allowed_by_cfg || allowed_by_list {
            continue;
        }
        if web_ctx.is_none() {
            failures.insert(
                key,
                "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes，且无法使用审批通道（例如非流式 Web 会话）。"
                    .to_string(),
            );
            continue;
        }
        let spec = crate::cm_internal::tool_approval::ApprovalRequestSpec {
            capability: crate::cm_internal::tool_approval::SensitiveCapability::OutboundHttpRead,
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
        let allow_handles = crate::cm_internal::tool_approval::SharedAllowlistHandles {
            web: web_ctx.map(|w| &w.persistent_allowlist_shared),
        };
        match crate::cm_internal::tool_approval::interactive_gate_after_whitelist_miss(
            web_ctx.map(crate::cm_internal::tool_approval::web_tool_runtime_approval_sink),
            &spec,
            "tool_registry::http_fetch approval parallel prefetch",
            &allow_handles,
        )
        .await
        {
            Ok(crate::cm_internal::tool_approval::InteractiveGateOutcome::Allowed) => {}
            Ok(crate::cm_internal::tool_approval::InteractiveGateOutcome::Denied(msg)) => {
                failures.insert(key, msg);
            }
            Err(crate::cm_internal::tool_approval::ToolApprovalWebError::ChannelUnavailable) => {
                failures.insert(key, "错误：审批通道不可用，请重试。".to_string());
            }
        }
    }
    failures
}
