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

// ── 非 `run_command` 工具的 async 后台门闩（`cargo_test` / `pytest_run`）────────

fn gate_with(enabled: bool, allow: &[&str], name: &str, args: &str) -> BackgroundAsyncGate {
    let set: std::collections::HashSet<String> =
        allow.iter().map(|s| (*s).to_string()).collect();
    background_async_gate(enabled, &set, name, args)
}

/// 最小可 `cargo test` 的临时工作区（无依赖 → 离线可编译）。
fn cargo_workspace() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"crabmate_async_probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir src");
    std::fs::write(dir.path().join("src/lib.rs"), "").expect("write lib.rs");
    dir
}

#[test]
fn background_async_gate_skips_when_not_applicable() {
    // 未显式请求 async → 不接管（保持既有「忽略未知参数」语义）。
    assert!(matches!(
        gate_with(true, &["cargo_test"], "cargo_test", r#"{"release":true}"#),
        BackgroundAsyncGate::Skip
    ));
    // `run_command` 走既有 `execute_run_command_async` 路径。
    assert!(matches!(
        gate_with(
            true,
            &["run_command"],
            "run_command",
            r#"{"command":"echo","async":true}"#
        ),
        BackgroundAsyncGate::Skip
    ));
    // 既不在装配表也不在白名单 → 不接管。
    assert!(matches!(
        gate_with(true, &[], "cargo_check", r#"{"async":true}"#),
        BackgroundAsyncGate::Skip
    ));
    // `async` 非布尔 / 参数非 JSON → 不接管。
    assert!(matches!(
        gate_with(true, &["cargo_test"], "cargo_test", r#"{"async":"yes"}"#),
        BackgroundAsyncGate::Skip
    ));
    assert!(matches!(
        gate_with(true, &["cargo_test"], "cargo_test", "not-json"),
        BackgroundAsyncGate::Skip
    ));
}

#[test]
fn background_async_gate_launch_and_deny_matrix() {
    assert!(matches!(
        gate_with(true, &["cargo_test"], "cargo_test", r#"{"async":true}"#),
        BackgroundAsyncGate::Launch
    ));
    let BackgroundAsyncGate::Deny(m) = gate_with(true, &[], "cargo_test", r#"{"async":true}"#) else {
        panic!("装配表内、白名单外应拒绝");
    };
    assert!(m.contains("background_job_async_tools"), "{m}");
    let BackgroundAsyncGate::Deny(m) =
        gate_with(true, &["cargo_check"], "cargo_check", r#"{"async":true}"#)
    else {
        panic!("白名单内、装配表外应拒绝");
    };
    assert!(m.contains("暂不支持后台执行"), "{m}");
    let BackgroundAsyncGate::Deny(m) =
        gate_with(false, &["cargo_test"], "cargo_test", r#"{"async":true}"#)
    else {
        panic!("总开关关闭应拒绝");
    };
    assert!(m.contains("未启用"), "{m}");
    // 未启用提示须同时点明白名单条件，避免用户只开总开关仍不解。
    assert!(m.contains("background_job_async_tools"), "{m}");
}

#[test]
fn background_async_assembly_cargo_test_reuses_foreground_argv() {
    let cfg = crate::cm_config::load_config(None).expect("embed default");
    let dir = cargo_workspace();
    let spawn = assemble_background_job_spawn(
        &cfg,
        "cargo_test",
        r#"{"release":true,"test_filter":"foo_bar"}"#,
        dir.path(),
    )
    .expect("装配 cargo test");
    assert_eq!(spawn.program, "cargo");
    assert_eq!(spawn.args, vec!["test", "--release", "foo_bar"]);
    assert_eq!(
        spawn.wall,
        std::time::Duration::from_secs(cfg.command_exec.command_timeout_secs.max(1))
    );
    // cwd 与前台 `build_cargo_subcommand_command` 一致（工作区根，不做 canonicalize）。
    assert_eq!(spawn.cwd, dir.path());
    // 无 Cargo.toml → 与前台同一条错误。
    let empty = tempfile::TempDir::new().expect("tempdir");
    let err = assemble_background_job_spawn(&cfg, "cargo_test", "{}", empty.path())
        .expect_err("无 Cargo.toml 应失败");
    assert!(err.contains("Cargo.toml"), "{err}");
    // 装配表外的工具：给「暂不支持」提示（门闩正常路径不会走到这里）。
    let err = assemble_background_job_spawn(&cfg, "not_a_tool", "{}", dir.path())
        .expect_err("未装配工具应失败");
    assert!(err.contains("暂不支持后台执行"), "{err}");
}

#[test]
fn background_async_assembly_pytest_uses_python3_module_pytest() {
    let cfg = crate::cm_config::load_config(None).expect("embed default");
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"probe\"\nversion = \"0.0.0\"\n",
    )
    .expect("write pyproject.toml");
    let spawn = assemble_background_job_spawn(&cfg, "pytest_run", r#"{"async":true}"#, dir.path())
        .expect("装配 pytest");
    assert_eq!(spawn.program, "python3");
    assert_eq!(spawn.args, vec!["-m", "pytest", "-q"]);
    // cwd 与前台一致：`build_pytest_command` 对工作区根做了 canonicalize。
    assert_eq!(spawn.cwd, dir.path().canonicalize().expect("canonicalize"));
    // `test_path` 校验与前台共用（越界路径在装配阶段即拒绝）。
    let err = assemble_background_job_spawn(
        &cfg,
        "pytest_run",
        r#"{"test_path":"../outside"}"#,
        dir.path(),
    )
    .expect_err("越界 test_path 应失败");
    assert!(err.contains("相对路径"), "{err}");
}

#[tokio::test]
async fn background_async_cargo_test_end_to_end_creates_job_and_reaches_terminal() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.tool_registry_policy.tool_registry_background_jobs_enabled = true;
    cfg.tool_registry_policy.tool_registry_background_job_async_tools = Arc::new(
        std::collections::HashSet::from(["cargo_test".to_string()]),
    );
    let cfg = Arc::new(cfg);
    let registry = crate::cm_internal::tool_jobs::registry_from_config(&cfg);
    let dir = cargo_workspace();
    // 过滤器不匹配任何测试 → `cargo test` 退出码 0。
    let args_json = r#"{"async":true,"test_filter":"no_such_test_zzz"}"#;
    let (out, inject) = try_dispatch_background_async_tool(
        &cfg,
        dir.path(),
        None,
        "cargo_test",
        args_json,
        Some(&registry),
    )
    .expect("门闩应接管 cargo_test 的 async 调用");
    let id = inject
        .as_ref()
        .and_then(|v| v.get("tool_job"))
        .and_then(|t| t.get("tool_job_id"))
        .and_then(|x| x.as_str())
        .expect("启动帧应含 tool_job_id")
        .to_string();
    assert!(out.contains(&id), "{out}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        let rec = registry.get(&id).expect("record");
        if rec.status.is_terminal() {
            assert_eq!(
                rec.status,
                crate::cm_internal::tool_jobs::JobStatus::Succeeded,
                "exit_code={:?} error_code={:?}",
                rec.outcome.as_ref().and_then(|o| o.exit_code),
                rec.outcome.as_ref().and_then(|o| o.error_code.clone())
            );
            // 证明 `cargo test` 真的跑起来了（而非空转成功）。
            let stdout = rec
                .outcome
                .as_ref()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            assert!(stdout.contains("test result"), "stdout: {stdout}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "等待 cargo_test 后台任务终态超时"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
