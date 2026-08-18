use super::*;

#[cfg(unix)]
#[test]
fn run_and_format_try_timeout_returns_timeout_error_with_partial_output() {
    let marker = format!("cm_cargo_runfmt_{}", std::process::id());
    let mut cmd = Command::new("bash");
    cmd.args([
        "-c",
        &format!("echo partial-cargo-out; sleep 60 # {marker}"),
    ]);
    let err = run_and_format_try(cmd, 4096, "cargo test", "cargo_test", Some(1))
        .expect_err("超时后应返回 Err");
    assert_eq!(err.code, "timeout");
    assert!(err.message.contains("命令执行超时"), "{:?}", err.message);
    assert!(
        err.message.contains("partial-cargo-out"),
        "{:?}",
        err.message
    );
    assert!(
        err.legacy_parsed.stdout.contains("partial-cargo-out"),
        "legacy_parsed 应保留部分 stdout: {:?}",
        err.legacy_parsed.stdout
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        !crate::cm_tools::subprocess_session::proc_cmdline_contains(&marker),
        "run_and_format_try 孙进程 sleep 仍在运行（进程组未杀干净）"
    );
}
