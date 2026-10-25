//! `run_command` 策略 gate 与 façade 分派的直接测试（拆出以降低 `execute_tests.rs` 物理行数）。

use super::tests::{
    approval_fixture, async_test_env, async_test_sandbox, contains_cmd, parsed_run_command,
    run_command_impl_for_test, test_policy_context,
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
