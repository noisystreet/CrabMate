//! 后台任务（装配表工具）的 `async` 门闩与装配测试。
//!
//! 拆出成独立文件：`execute_tests.rs` 已顶到文件行数棘轮（≤ 920 行），
//! 与 `execute_run_command_gate_tests.rs` / `execute_read_dir_gate_tests.rs` 同模式。

use super::background_job::{
    BackgroundAsyncGate, assemble_background_job_spawn, background_async_gate,
    try_dispatch_background_async_tool,
};
use std::collections::HashSet;
use std::sync::Arc;

// ── 非 `run_command` 工具的 async 后台门闩（装配表：cargo 家族 / `pytest_run`）────

fn gate_with(enabled: bool, allow: &[&str], name: &str, args: &str) -> BackgroundAsyncGate {
    let set: HashSet<String> = allow.iter().map(|s| (*s).to_string()).collect();
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
    // 样本工具用装配表之外的（`cargo_check` 已入表，不能再当反例）。
    assert!(matches!(
        gate_with(true, &[], "docs_health_sweep", r#"{"async":true}"#),
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
    // 白名单内、装配表外 → 拒绝（样本用装配表之外的工具）。
    let BackgroundAsyncGate::Deny(m) =
        gate_with(true, &["docs_health_sweep"], "docs_health_sweep", r#"{"async":true}"#)
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
fn background_async_assembly_cargo_check_clippy_run_reuse_foreground_argv() {
    let cfg = crate::cm_config::load_config(None).expect("embed default");
    let dir = cargo_workspace();
    // `cargo_check` / `cargo_clippy` 共用 `CargoCommonCliArgs`；`async` 只是门闩信号，不进 CLI。
    let spawn = assemble_background_job_spawn(
        &cfg,
        "cargo_check",
        r#"{"async":true,"release":true,"all_targets":true,"package":"crabmate"}"#,
        dir.path(),
    )
    .expect("装配 cargo check");
    assert_eq!(spawn.program, "cargo");
    assert_eq!(
        spawn.args,
        vec!["check", "--release", "--all-targets", "--package", "crabmate"]
    );
    // cwd 与前台 `build_cargo_subcommand_command` 一致（工作区根，不做 canonicalize）。
    assert_eq!(spawn.cwd, dir.path());

    let spawn = assemble_background_job_spawn(
        &cfg,
        "cargo_clippy",
        r#"{"all_targets":true}"#,
        dir.path(),
    )
    .expect("装配 cargo clippy");
    assert_eq!(spawn.args, vec!["clippy", "--all-targets"]);

    // `cargo_run` 用 `CargoRunArgs`：`args` 透传在 `--` 之后。
    let spawn = assemble_background_job_spawn(
        &cfg,
        "cargo_run",
        r#"{"bin":"probe","args":["--help"]}"#,
        dir.path(),
    )
    .expect("装配 cargo run");
    assert_eq!(
        spawn.args,
        vec!["run", "--bin", "probe", "--", "--help"]
    );
    assert!(
        !spawn.args.iter().any(|a| a == "async"),
        "async 不得进入 cargo argv：{:?}",
        spawn.args
    );

    // 门闩：三个工具都在装配表内，故总开关关闭时给出「未启用」而非被静默忽略。
    let set = std::collections::HashSet::from([
        "cargo_check".to_string(),
        "cargo_clippy".to_string(),
        "cargo_run".to_string(),
    ]);
    for name in ["cargo_check", "cargo_clippy", "cargo_run"] {
        assert!(
            matches!(
                background_async_gate(true, &set, name, r#"{"async":true}"#),
                BackgroundAsyncGate::Launch
            ),
            "{name} 应在装配表 ∪ 白名单内可发起"
        );
        assert!(
            matches!(
                background_async_gate(true, &HashSet::new(), name, r#"{"async":true}"#),
                BackgroundAsyncGate::Deny(_)
            ),
            "{name} 不在白名单时应 Deny（白名单只能收敛）"
        );
    }
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
    let registry = crate::jobs::tool_jobs::registry_from_config(&cfg);
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
                crate::jobs::tool_jobs::JobStatus::Succeeded,
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
