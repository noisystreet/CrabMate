//! `run_command` 策略 gate 与 façade 分派的直接测试（拆出以降低 `execute_tests.rs` 物理行数）。

use super::tests::{
    approval_fixture, async_test_env, async_test_sandbox, contains_cmd, evaluate_for_test,
    parsed_run_command, run_command_impl_for_test, test_policy_context, test_policy_context_with_caps,
};
use super::*;

/// 直接调用 shell-wrap gate，避免前置未知命令 gate 掩盖本 gate 的回归。
#[tokio::test]
async fn posix_shell_wrap_gate_direct_matrix() {
    use CommandApprovalDecision::{AllowOnce, Deny};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["echo".to_string()].into();
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"echo","args":["$HOME"]}"#);
    let allowed: Arc<[String]> = vec!["echo".to_string()].into();

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let (effective, approval) = approve_posix_shell_wrap_if_needed(
        &parsed,
        &parsed.spec.command,
        &parsed.spec.args,
        Arc::clone(&allowed),
        &ctx,
    )
    .await
    .expect("allow once");
    assert!(contains_cmd(&effective, "bash"), "{effective:?}");
    assert_eq!(approval, Some(PolicyApproval::PosixShellWrap));

    let fixture = approval_fixture(Deny);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    let (msg, _) = approve_posix_shell_wrap_if_needed(
        &parsed,
        &parsed.spec.command,
        &parsed.spec.args,
        Arc::clone(&allowed),
        &ctx,
    )
    .await
    .expect_err("deny")
    .into_message();
    assert!(msg.contains("已拒绝"), "{msg}");

    let fixture = approval_fixture(AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Async);
    let (msg, _) = approve_posix_shell_wrap_if_needed(
        &parsed,
        &parsed.spec.command,
        &parsed.spec.args,
        allowed,
        &ctx,
    )
    .await
    .expect_err("async")
    .into_message();
    assert!(msg.contains("async"), "{msg}");
}

/// 直接调用工作区外路径 gate，覆盖 Docker / async / 批准三态。
#[tokio::test]
async fn external_path_gate_direct_docker_and_async() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"cat","args":["/etc/passwd"]}"#);
    let allowed = vec!["cat".to_string()];

    let ctx = test_policy_context(&cfg, None, ExecMode::Docker);
    let (msg, _) = approve_external_run_command_paths_if_needed(&parsed, &allowed, &ctx)
        .await
        .expect_err("docker")
        .into_message();
    assert!(msg.contains("Docker"), "{msg}");

    let fixture = approval_fixture(CommandApprovalDecision::AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Async);
    let (msg, _) = approve_external_run_command_paths_if_needed(&parsed, &allowed, &ctx)
        .await
        .expect_err("async")
        .into_message();
    assert!(msg.contains("async"), "{msg}");

    let fixture = approval_fixture(CommandApprovalDecision::AllowOnce);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    assert_eq!(
        approve_external_run_command_paths_if_needed(&parsed, &allowed, &ctx)
            .await
            .expect("allow once"),
        ExternalPathGate::Approved
    );
}

/// Docker 同步沙盒下，工作区外路径必须在转交容器前被 policy 拒绝。
#[tokio::test]
async fn docker_mode_rejects_external_path_before_sandbox_dispatch() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode = SyncDefaultToolSandboxMode::Docker;
    cfg.command_exec.allow_external_path_with_approval = true;
    let cfg = Arc::new(cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    let (out, inject) = run_command_impl_for_test(
        &env,
        wd,
        None,
        None,
        r#"{"command":"cat","args":["/etc/passwd"]}"#,
    )
    .await;
    assert!(out.contains("Docker"), "{out}");
    assert!(inject.is_none());
    drop(sandbox);
}

/// façade 对 `ExecMode::Pty` 防御性拒绝（PTY 仅由 `terminal_session` 使用）。
#[tokio::test]
async fn facade_rejects_pty_mode() {
    let cfg = Arc::new(crate::cm_config::load_config(None).expect("embed default"));
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let mut workspace_changed = false;
    let args = r#"{"command":"echo","args":["hi"]}"#;
    // R3：`PolicyDecision` 只借用命令能力快照；本用例只验证 PTY 分派，就地构造一份。
    let policy_caps = RunCommandCaps {
        allowed_commands: vec!["echo".to_string()].into(),
    };
    let (out, inject) = execute_run_command_facade(RunCommandFacadeInvoke {
        env: &env,
        effective_working_dir: std::path::Path::new("."),
        workspace_is_set: true,
        workspace_changed: &mut workspace_changed,
        web_ctx: None,
        args,
        tool_call_id: "tc_pty",
        sse_out_tx: None,
        sse_control_mirror: None,
        cancel: None,
        tool_jobs: None,
        timeout_secs: None,
        parsed: parsed_run_command(args),
        decision: PolicyDecision {
            caps: &policy_caps,
            skip_arg_safety: false,
            capabilities: PolicyCapabilities {
                mode: ExecMode::Pty,
                has_approval_channel: false,
            },
            approvals: Vec::new(),
        },
        capabilities: RunCommandCapabilities {
            mode: ExecMode::Pty,
            background_jobs_enabled: true,
            has_job_registry: false,
            docker_sandbox: false,
        },
    })
    .await;
    assert!(out.contains("PTY"), "{out}");
    assert!(inject.is_none());
    drop(sandbox);
}

/// S3：L2（`approval = Deny`）下未知命令**直接拒**——即使审批通道可用也不弹窗。
#[tokio::test]
async fn l2_caps_denies_unknown_command_without_prompting() {
    use crate::cm_agent::permission::project_caps;
    use crate::cm_types::PermissionLevel;

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let caps = project_caps(PermissionLevel::Workspace, &cfg);
    let parsed = parsed_run_command(r#"{"command":"crabmate_l2_probe","args":[]}"#);

    // 审批通道存在——L2 也不该用它，而是硬拒。
    let fixture = approval_fixture(CommandApprovalDecision::AllowOnce);
    let ctx = test_policy_context_with_caps(&cfg, &caps, Some(&fixture.web), ExecMode::Sync);
    let mut snapshot = ctx.command_caps();
    let (msg, _) = evaluate_policy(&parsed, &ctx, &mut snapshot)
        .await
        .expect_err("L2 未知命令应被拒")
        .into_message();
    assert!(msg.contains("不在当前权限等级"), "{msg}");
    assert!(msg.contains("workspace"), "{msg}");
    assert_eq!(
        snapshot.allowed_commands.as_ref(),
        cfg.command_exec.allowed_commands.as_ref(),
        "失败路径不得改写快照（L2 基白名单来自 caps 的 Allowlist）"
    );
}

/// S3：L4（`command_scope = Any`）下未知命令直接放行，**不**需要审批通道。
#[tokio::test]
async fn l4_caps_allows_unknown_command_without_approval_channel() {
    use crate::cm_agent::permission::project_caps;
    use crate::cm_types::PermissionLevel;

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let caps = project_caps(PermissionLevel::Unrestricted, &cfg);
    let parsed = parsed_run_command(r#"{"command":"crabmate_l4_probe","args":[]}"#);

    let ctx = test_policy_context_with_caps(&cfg, &caps, None, ExecMode::Sync);
    let mut snapshot = ctx.command_caps();
    let decision = evaluate_policy(&parsed, &ctx, &mut snapshot)
        .await
        .expect("L4 不应需要审批通道");
    assert!(
        crate::cm_tools::tools::allowlist_allows_all(&decision.caps.allowed_commands),
        "{:?}",
        decision.caps.allowed_commands
    );
}

/// `run_command` 策略 gate 的结果摘要，用于「接线 vs 未接线」的逐字比较。
///
/// `Ok` 带上生效白名单与 `skip_arg_safety`（放行路径的两个可见结论）；`Err` 带用户可见正文
/// （拒绝路径的唯一对外形状）。刻意不比较内部枚举：正文与白名单就是模型/用户能观察到的东西。
type GateOutcome = Result<(Vec<String>, bool), String>;

async fn gate_outcome(
    cfg: &Arc<crate::cm_config::AgentConfig>,
    caps: Option<&crate::cm_agent::permission::PermissionCaps>,
    web: Option<&crate::cm_tools::tool_runtime::WebToolRuntime>,
    parsed: &ParsedRunCommandJson,
    mode: ExecMode,
) -> GateOutcome {
    let ctx = match caps {
        Some(c) => test_policy_context_with_caps(cfg, c, web, mode),
        None => test_policy_context(cfg, web, mode),
    };
    match evaluate_for_test(parsed, &ctx).await {
        Ok(d) => Ok((d.allowed_commands.to_vec(), d.skip_arg_safety)),
        Err(e) => Err(e.into_message().0),
    }
}

/// S3 验收：L3（`approved`）的 `caps` 接线与「未接线（`permission = None`，回落 `cfg`）」
/// **逐字等价**——三道 gate 的结论、白名单扩展、弹窗次数与错误正文都不因接线而改变。
///
/// 设计 §8.1 的接线原则是「只换入参来源，不重写 gate 逻辑」，而 L3 的投影本就与 `cfg` 同源
/// （白名单 / 前缀 / `Gate` 审批），所以这是唯一能钉住「S3 不偷偷改变现状行为」的对照。
#[tokio::test]
async fn l3_caps_matches_unwired_fallback() {
    use crate::cm_agent::permission::project_caps;
    use crate::cm_types::PermissionLevel;
    use CommandApprovalDecision::{AllowOnce, Deny};

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    // 固定的窄白名单（不受外部 `CM_ALLOWED_COMMANDS` 影响），且不含 `bash` / `cat`：
    // 下面要验证「批准后扩展白名单」与「外部路径走审批」两条现状路径。
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    cfg.command_exec.allow_external_path_with_approval = true;
    let cfg = Arc::new(cfg);
    let caps = project_caps(PermissionLevel::Approved, &cfg);

    // (1) 白名单内命令：两条路径都不弹窗。
    let parsed = parsed_run_command(r#"{"command":"cargo","args":["--version"]}"#);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), None, &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, None, &parsed, ExecMode::Sync).await,
        "L3 白名单内命令应与未接线逐字一致"
    );

    // (2) 未知命令 + 无审批通道：同一「审批通道不可用」结论。
    let parsed = parsed_run_command(r#"{"command":"crabmate_l3_probe","args":[]}"#);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), None, &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, None, &parsed, ExecMode::Sync).await,
        "L3 未知命令的失败正文应与未接线一致"
    );

    // (3) 未知命令 + AllowOnce：同样把命令纳入生效白名单（快照扩展点相同）。
    let wired = approval_fixture(AllowOnce);
    let unwired = approval_fixture(AllowOnce);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), Some(&wired.web), &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, Some(&unwired.web), &parsed, ExecMode::Sync).await,
        "L3 的 AllowOnce 放行结论应与未接线一致"
    );

    // (4) 未知命令 + 用户拒绝：同一拒绝正文。
    let wired = approval_fixture(Deny);
    let unwired = approval_fixture(Deny);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), Some(&wired.web), &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, Some(&unwired.web), &parsed, ExecMode::Sync).await,
        "L3 的用户拒绝正文应与未接线一致"
    );

    // (5) POSIX shell 包装（`$HOME`）+ AllowOnce：同样补 `bash` 进白名单。
    let parsed = parsed_run_command(r#"{"command":"cargo","args":["$HOME"]}"#);
    let wired = approval_fixture(AllowOnce);
    let unwired = approval_fixture(AllowOnce);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), Some(&wired.web), &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, Some(&unwired.web), &parsed, ExecMode::Sync).await,
        "L3 的 shell 包装审批结论应与未接线一致"
    );

    // (6) 工作区外路径 + AllowOnce：同样得到 `skip_arg_safety = true`。
    let parsed = parsed_run_command(r#"{"command":"cat","args":["/etc/passwd"]}"#);
    let wired = approval_fixture(AllowOnce);
    let unwired = approval_fixture(AllowOnce);
    assert_eq!(
        gate_outcome(&cfg, Some(&caps), Some(&wired.web), &parsed, ExecMode::Sync).await,
        gate_outcome(&cfg, None, Some(&unwired.web), &parsed, ExecMode::Sync).await,
        "L3 的外部路径审批结论应与未接线一致"
    );
}

#[tokio::test]
async fn external_run_command_gate_not_needed_when_disabled_or_safe_args() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    let external = parsed_run_command(r#"{"command":"cat","args":["/etc/passwd"]}"#);
    let git_range = parsed_run_command(r#"{"command":"git","args":["log","main..HEAD"]}"#);

    cfg.command_exec.allow_external_path_with_approval = false;
    let cfg = Arc::new(cfg);
    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let d = evaluate_for_test(&external, &ctx)
        .await
        .expect("disabled → NotNeeded");
    assert!(!d.skip_arg_safety);

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let cfg = Arc::new(cfg);
    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let d = evaluate_for_test(&git_range, &ctx)
        .await
        .expect("git range → NotNeeded");
    assert!(!d.skip_arg_safety);
}

#[tokio::test]
async fn external_run_command_gate_errs_without_channel_or_in_docker() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let mut cfg = Arc::new(cfg);
    let abs = parsed_run_command(r#"{"command":"cat","args":["/etc/passwd"]}"#);

    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&abs, &ctx)
        .await
        .expect_err("no channel")
        .into_message();
    assert!(msg.contains("需要审批通道"), "{msg}");

    Arc::get_mut(&mut cfg)
        .expect("sole owner")
        .sync_tool_sandbox
        .sync_default_tool_sandbox_mode = crate::cm_config::SyncDefaultToolSandboxMode::Docker;
    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&abs, &ctx)
        .await
        .expect_err("docker")
        .into_message();
    assert!(msg.contains("Docker"), "{msg}");
}

#[tokio::test]
async fn posix_shell_wrap_gate_errs_without_channel() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["echo".to_string()].into();
    let cfg = Arc::new(cfg);
    let parsed = parsed_run_command(r#"{"command":"echo","args":["$HOME"]}"#);

    let ctx = test_policy_context(&cfg, None, ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("no channel")
        .into_message();
    assert!(msg.contains("无审批通道"), "{msg}");
    assert!(msg.contains("echo $HOME"), "{msg}");
}
