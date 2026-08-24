//! `execute` 模块单元测试（拆出以降低 `execute.rs` 物理行数棘轮）。

use super::super::meta::HandlerLookupTable;
use super::*;
use std::sync::Arc;

use crate::cm_types::{FunctionCall, ToolCall};

fn tool_call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: "tc_1".to_string(),
        typ: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
    }
}

#[test]
fn read_dir_path_is_external_detects_absolute_and_parent_ref() {
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"/tmp"}"#),
        Some("/tmp".to_string())
    );
    assert_eq!(
        read_dir_path_is_external(r#"{"path":"../secrets"}"#),
        Some("../secrets".to_string())
    );
    assert_eq!(read_dir_path_is_external(r#"{"path":"src"}"#), None);
}

#[tokio::test]
async fn prefetch_parallel_syncdefault_approvals_blocks_external_read_dir_without_channel() {
    let calls = vec![tool_call("read_dir", r#"{"path":"/tmp"}"#)];
    let failures = prefetch_parallel_syncdefault_approvals(
        &calls,
        None,
        &HandlerLookupTable::default_dispatch(),
    )
    .await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("read_dir".to_string(), r#"{"path":"/tmp"}"#.to_string()))
        .expect("missing failure for external read_dir");
    assert!(msg.contains("需要审批通道"));
}

#[tokio::test]
async fn prefetch_http_fetch_blocks_without_channel_when_prefix_miss() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.http_fetch.http_fetch_allowed_prefixes.clear();
    let cfg = Arc::new(cfg);
    let args = r#"{"url":"https://example.com/private"}"#;
    let calls = vec![tool_call("http_fetch", args)];
    let failures = prefetch_http_fetch_parallel_approvals(&calls, &cfg, None).await;
    assert_eq!(failures.len(), 1);
    let msg = failures
        .get(&("http_fetch".to_string(), args.to_string()))
        .expect("missing failure for http_fetch");
    assert!(msg.contains("http_fetch_allowed_prefixes"));
    assert!(msg.contains("审批通道"));
}

#[tokio::test]
async fn prefetch_http_fetch_skips_approval_when_prefix_matches() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.http_fetch.http_fetch_allowed_prefixes =
        vec!["https://example.com/".to_string()];
    let cfg = Arc::new(cfg);
    let args = r#"{"url":"https://example.com/api/v1"}"#;
    let calls = vec![tool_call("http_fetch", args)];
    let failures = prefetch_http_fetch_parallel_approvals(&calls, &cfg, None).await;
    assert!(failures.is_empty());
}

#[tokio::test]
async fn external_run_command_gate_not_needed_when_disabled_or_safe_args() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    let allowed = cfg.command_exec.allowed_commands.to_vec();
    let wd = std::path::Path::new(".");

    cfg.command_exec.allow_external_path_with_approval = false;
    let g = approve_external_run_command_paths_if_needed(
        &cfg,
        r#"{"command":"cat","args":["/etc/passwd"]}"#,
        wd,
        &allowed,
        None,
        "run_command",
        false,
    )
    .await
    .expect("disabled → NotNeeded");
    assert_eq!(g, ExternalPathGate::NotNeeded);

    cfg.command_exec.allow_external_path_with_approval = true;
    let g = approve_external_run_command_paths_if_needed(
        &cfg,
        r#"{"command":"git","args":["log","main..HEAD"]}"#,
        wd,
        &allowed,
        None,
        "run_command",
        false,
    )
    .await
    .expect("git range → NotNeeded");
    assert_eq!(g, ExternalPathGate::NotNeeded);
}

#[tokio::test]
async fn external_run_command_gate_errs_without_channel_or_in_docker() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allow_external_path_with_approval = true;
    let allowed = cfg.command_exec.allowed_commands.to_vec();
    let wd = std::path::Path::new(".");
    let abs = r#"{"command":"cat","args":["/etc/passwd"]}"#;

    let err =
        approve_external_run_command_paths_if_needed(&cfg, abs, wd, &allowed, None, "run_command", false)
            .await
            .expect_err("no channel");
    assert!(err.contains("需要审批通道"), "{err}");

    cfg.sync_tool_sandbox.sync_default_tool_sandbox_mode =
        crate::cm_config::SyncDefaultToolSandboxMode::Docker;
    let err =
        approve_external_run_command_paths_if_needed(&cfg, abs, wd, &allowed, None, "run_command", false)
            .await
            .expect_err("docker");
    assert!(err.contains("Docker"), "{err}");
}

#[tokio::test]
async fn posix_shell_wrap_gate_errs_without_channel() {
    let allowed: Arc<[String]> = vec!["echo".to_string()].into();
    let err = approve_posix_shell_wrap_if_needed(
        "echo",
        &["$HOME".to_string()],
        "echo $HOME",
        allowed,
        None,
        "run_command",
        false,
    )
    .await
    .expect_err("no channel");
    assert!(err.contains("无审批通道"), "{err}");
    assert!(err.contains("echo $HOME"), "{err}");
}

#[tokio::test]
async fn posix_shell_wrap_skips_glob_when_bash_allowlisted() {
    let allowed: Arc<[String]> = vec!["ls".to_string(), "bash".to_string()].into();
    approve_posix_shell_wrap_if_needed(
        "ls",
        &["*.rs".to_string()],
        "ls *.rs",
        allowed,
        None,
        "run_command",
        false,
    )
    .await
    .expect("glob + bash → no extra approval");
}

#[tokio::test]
async fn posix_shell_wrap_skips_cd_prefix_without_expansion() {
    let allowed: Arc<[String]> = vec!["cd".into(), "git".into(), "bash".into()].into();
    let (cmd, args) = tools::peel_cd_prefix_argv_for_shell_policy(
        "cd",
        &[
            "src".to_string(),
            "&&".to_string(),
            "git".to_string(),
            "status".to_string(),
        ],
    );
    approve_posix_shell_wrap_if_needed(
        &cmd,
        &args,
        "cd src && git status",
        allowed,
        None,
        "run_command",
        false,
    )
    .await
    .expect("cd peel → git status");
}

#[test]
fn posix_shell_wrap_web_operators_need_approval_even_with_bash() {
    assert!(posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["&&".to_string(), "pwd".to_string()],
        true,
        true,
    ));
    assert!(!posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["&&".to_string(), "pwd".to_string()],
        true,
        false,
    ));
    assert!(!posix_shell_wrap_needs_interactive_approval(
        "ls",
        &["*.rs".to_string()],
        true,
        true,
    ));
}

#[test]
fn run_command_chunk_seq_is_monotonic_on_mirror() {
    use crate::cm_sse_protocol::sse::protocol::SsePayload;
    use crate::cm_tools::subprocess_session::SessionStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    let seen = Arc::new(Mutex::new(Vec::<u64>::new()));
    let seen_cb = Arc::clone(&seen);
    let mirror: crate::cm_sse_protocol::sse::SseControlMirror = Arc::new(move |p| {
        if let SsePayload::ToolOutputChunk { tool_output_chunk } = p {
            seen_cb.lock().expect("lock").push(tool_output_chunk.seq);
        }
    });
    let seq = AtomicU64::new(0);
    let utf8 = Mutex::new(Vec::<u8>::new());
    assert!(emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stdout,
        b"a\n",
        None,
        Some(&mirror),
        &utf8,
    ));
    assert!(emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stderr,
        b"e\n",
        None,
        Some(&mirror),
        &utf8,
    ));
    assert_eq!(*seen.lock().expect("lock"), vec![1, 2]);
}

#[tokio::test]
async fn run_command_chunk_try_send_full_does_not_bump_seq() {
    use crate::cm_tools::subprocess_session::SessionStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;
    use tokio::sync::mpsc;

    let seq = AtomicU64::new(0);
    let utf8 = Mutex::new(Vec::<u8>::new());
    let (tx, _rx) = mpsc::channel::<String>(1);
    tx.try_send("fill".into()).expect("fill");
    assert!(!emit_run_command_tool_output_chunk(
        &seq,
        "tc-p1",
        SessionStream::Stdout,
        b"lost?\n",
        Some(&tx),
        None,
        &utf8,
    ));
    assert_eq!(seq.load(std::sync::atomic::Ordering::SeqCst), 0);
}

fn async_test_sandbox(
) -> Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend> {
    crate::cm_internal::tool_sandbox::default_sync_default_sandbox_backend()
}

fn async_test_env<'a>(
    cfg: &'a Arc<AgentConfig>,
    sandbox: &'a Arc<dyn crate::cm_internal::tool_sandbox::SyncDefaultSandboxBackend>,
) -> ToolExecEnv<'a> {
    ToolExecEnv {
        cfg,
        sandbox_backend: sandbox,
    }
}

#[tokio::test]
async fn run_command_async_disabled_returns_invalid_args() {
    let cfg = Arc::new(crate::cm_config::load_config(None).expect("embed default"));
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    let (out, inject) = execute_run_command_async(RunCommandAsyncInvoke {
        env: &env,
        effective_working_dir: wd,
        web_ctx: None,
        args: r#"{"command":"echo","args":["hi"],"async":true}"#,
        tool_jobs: None,
    })
    .await;
    assert!(out.contains("未启用"), "{out}");
    assert!(inject.is_none());
    drop(sandbox);
}

#[tokio::test]
async fn run_command_async_enabled_creates_job_and_returns_start_frame() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy.tool_registry_background_jobs_enabled = true;
    let cfg = Arc::new(cfg);
    let registry = crate::cm_internal::tool_jobs::registry_from_config(&cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    let (out, inject) = execute_run_command_async(RunCommandAsyncInvoke {
        env: &env,
        effective_working_dir: wd,
        web_ctx: None,
        args: r#"{"command":"echo","args":["async-hi"],"async":true,"timeout_secs":5}"#,
        tool_jobs: Some(Arc::clone(&registry)),
    })
    .await;
    let tj = inject
        .as_ref()
        .and_then(|v| v.get("tool_job"))
        .expect("tool_job 启动帧");
    let id = tj.get("tool_job_id").and_then(|x| x.as_str()).expect("id");
    assert!(id.starts_with("tooljob_"));
    assert_eq!(
        tj.get("tool_job_status").and_then(|x| x.as_str()),
        Some("queued")
    );
    assert_eq!(
        tj.get("tool_job_poll_url").and_then(|x| x.as_str()),
        Some(format!("/tools/jobs/{id}").as_str())
    );
    assert!(out.contains(id), "{out}");
    // 等待 worker 完成（async 已启动），校验注册表落定终态。
    let id_owned = id.to_string();
    loop {
        let rec = registry.get(&id_owned).expect("record");
        if rec.status.is_terminal() {
            assert_eq!(rec.status, crate::cm_internal::tool_jobs::JobStatus::Succeeded);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    drop(sandbox);
}

#[tokio::test]
async fn run_command_async_rejects_allow_once_requiring_command() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy.tool_registry_background_jobs_enabled = true;
    let cfg = Arc::new(cfg);
    let registry = crate::cm_internal::tool_jobs::registry_from_config(&cfg);
    let sandbox = async_test_sandbox();
    let env = async_test_env(&cfg, &sandbox);
    let wd = std::path::Path::new(".");
    // `definitely_not_allowlisted_cmd_xyz` 不在白名单且无审批通道（web_ctx=None）→ 拒绝 async。
    let (out, inject) = execute_run_command_async(RunCommandAsyncInvoke {
        env: &env,
        effective_working_dir: wd,
        web_ctx: None,
        args: r#"{"command":"definitely_not_allowlisted_cmd_xyz","async":true}"#,
        tool_jobs: Some(Arc::clone(&registry)),
    })
    .await;
    assert!(out.contains("AllowAlways"), "{out}");
    assert!(inject.is_none());
    assert_eq!(registry.stats().total, 0, "拒绝后不得创建任务");
    drop(sandbox);
}
