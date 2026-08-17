use super::*;
use std::path::Path;

const TEST_MAX_OUTPUT_LEN: usize = 8192;
const TEST_ALLOWED: &[&str] = &[
    "ls",
    "pwd",
    "whoami",
    "date",
    "echo",
    "id",
    "uname",
    "env",
    "df",
    "du",
    "head",
    "tail",
    "wc",
    "cat",
    "cd",
    "cmake",
    "ninja",
    "gcc",
    "g++",
    "clang",
    "clang++",
    "c++filt",
    "autoreconf",
    "autoconf",
    "automake",
    "aclocal",
    "make",
    "cargo",
    "bash",
    "sh",
    "sleep",
];

fn test_allowed() -> Vec<String> {
    TEST_ALLOWED.iter().map(|s| s.to_string()).collect()
}

fn test_work_dir() -> &'static Path {
    Path::new(".")
}

#[test]
fn test_run_invalid_json() {
    let out = run(
        "not json",
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.starts_with("参数解析错误"));
}

#[test]
fn test_run_missing_command_checked() {
    let e = run_checked(
        r#"{"args":[]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    )
    .expect_err("missing command");
    assert_eq!(e.kind(), "missing_command");
}

#[test]
fn test_run_missing_command() {
    let out = run(
        r#"{"args":[]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert_eq!(out, "错误：缺少 command 参数");
}

#[test]
fn test_run_disallowed_command_checked() {
    let e = run_checked(
        r#"{"command":"rm","args":["-rf","/"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    )
    .expect_err("disallowed");
    assert_eq!(e.kind(), "disallowed_command");
    let msg = e.user_message();
    assert!(msg.contains("不允许的命令"));
    assert!(msg.contains("rm"));
}

#[test]
fn test_run_disallowed_command() {
    let out = run(
        r#"{"command":"rm","args":["-rf","/"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("不允许的命令"));
    assert!(out.contains("rm"));
}

#[test]
fn test_run_args_not_array() {
    let out = run(
        r#"{"command":"echo","args":"x"}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("args 必须是字符串数组"));
}

#[test]
fn test_run_unsafe_arg_absolute_path() {
    let out = run(
        r#"{"command":"cat","args":["/etc/passwd"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("参数不允许"));
}

#[test]
fn test_run_unsafe_arg_parent_dir() {
    let out = run(
        r#"{"command":"cat","args":["../../etc/passwd"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("参数不允许"));
}

#[test]
fn test_run_workspace_absolute_arg_auto_normalized() {
    let wd = std::env::current_dir().expect("cwd");
    let wd_abs = wd.to_string_lossy().to_string();
    let out = run(
        &format!(r#"{{"command":"ls","args":["{wd_abs}"]}}"#),
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        &wd,
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
}

#[test]
fn prepare_peels_cd_prefix_into_effective_workdir() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"command":"cd","args":["src","&&","echo","peeled"]}"#)
            .expect("json");
    let p =
        prepare_run_command_invocation(&v, Path::new("."), &test_allowed(), false).expect("prep");
    assert_eq!(p.cmd_name, "echo");
    assert_eq!(p.cmd_args, vec!["peeled".to_string()]);
    assert!(
        p.effective_working_dir.ends_with("src"),
        "{:?}",
        p.effective_working_dir
    );
}

#[test]
fn prepare_cd_without_and_is_rejected() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"command":"cd","args":["src"]}"#).expect("json");
    let e = prepare_run_command_invocation(&v, Path::new("."), &test_allowed(), false)
        .err()
        .expect("cd alone");
    assert_eq!(e.kind(), "cd_prefix_invalid");
}

#[test]
fn prepare_splits_embedded_command_prefix() {
    let v = serde_json::from_str::<serde_json::Value>(
        r#"{"command":"pre-commit run --all-files","args":[]}"#,
    )
    .expect("json");
    let p = prepare_run_command_invocation(&v, Path::new("."), &["pre-commit".to_string()], false)
        .expect("prep");
    assert_eq!(p.cmd_name, "pre-commit");
    assert_eq!(
        p.cmd_args,
        vec!["run".to_string(), "--all-files".to_string()]
    );
}

#[test]
fn prepare_merges_dot_slash_command_with_single_relative_executable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let bin_dir = dir.path().join("hello/build");
    std::fs::create_dir_all(&bin_dir).expect("mkdir");
    let bin = bin_dir.join("hello");
    std::fs::write(&bin, b"\x7fELF").expect("write");
    let mut perms = std::fs::metadata(&bin).expect("meta").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).expect("chmod");

    let v: serde_json::Value =
        serde_json::from_str(r#"{"command":"./","args":["hello/build/hello"]}"#).expect("json");
    let p = prepare_run_command_invocation(&v, dir.path(), &[], false).expect("prep");
    assert_eq!(p.cmd_raw, "./hello/build/hello");
    assert!(p.cmd_args.is_empty());
    assert!(
        p.exec_path.is_some(),
        "merged path should resolve as workspace executable"
    );
}

#[test]
fn run_command_embedded_args_in_command_field() {
    let out = run(
        r#"{"command":"echo hello world","args":[]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("hello world"), "{out}");
}

#[test]
fn run_command_embedded_prefix_then_json_args_order() {
    let out = run(
        r#"{"command":"echo a","args":["b"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("a b"), "{out}");
}

#[test]
fn command_not_found_extended_appends_install_hint() {
    let e = RunCommandError::CommandNotFound {
        cmd: "python3".to_string(),
        work_dir: "/tmp".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "x"),
    };
    let s = e.extended_user_message();
    assert!(s.contains("安装提示"), "{s}");
    assert!(s.contains("python3 --version"), "{s}");
}

#[test]
fn command_not_found_extended_skips_hint_for_unknown_cmd() {
    let e = RunCommandError::CommandNotFound {
        cmd: "crabmate_nonexistent_cli_9f3a".to_string(),
        work_dir: "/tmp".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "x"),
    };
    let s = e.extended_user_message();
    assert!(!s.contains("安装提示"), "{s}");
}

#[test]
fn skip_arg_safety_allows_external_absolute_path() {
    let out = run(
        r#"{"command":"ls","args":["/tmp"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        true,
    );
    assert!(
        out.contains("退出码：") || out.contains("标准输出"),
        "approved external path should reach exec: {out}"
    );
}

#[test]
fn skip_arg_safety_false_still_rejects_external_path() {
    let e = run_checked(
        r#"{"command":"ls","args":["/tmp"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
    )
    .expect_err("unsafe");
    assert_eq!(e.kind(), "unsafe_arg");
}

#[test]
fn without_bash_dollar_var_is_rejected() {
    let allowed: Vec<String> = test_allowed()
        .into_iter()
        .filter(|c| !c.eq_ignore_ascii_case("bash") && !c.eq_ignore_ascii_case("sh"))
        .collect();
    let e = run_checked(
        r#"{"command":"echo","args":["$HOME"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    )
    .expect_err("shell var");
    assert_eq!(e.kind(), "shell_variable_detected");
}

#[test]
fn bash_on_allowlist_wraps_and_expands_home() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"echo","args":["$HOME"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("命令：echo $HOME"), "{out}");
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        assert!(out.contains(&home), "{out}");
    }
}

#[test]
fn bash_c_script_with_substitution_is_allowed() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"bash","args":["-c","echo hi"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("hi"), "{out}");
}

#[test]
fn bash_wraps_operators_and_gh_dollar_keeps_token_flag() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    allowed.push("gh".into());
    let out = run(
        r#"{"command":"ls","args":["&&","pwd"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("命令：ls && pwd"), "{out}");

    let p = prepare_run_command_for_pty_spawn(
        r#"{"command":"gh","args":["api","repos/$ORG/x"]}"#,
        test_work_dir(),
        &allowed,
        false,
    )
    .expect("wrap gh");
    assert!(p.inject_gh_token);
    assert_eq!(p.cmd_name, "bash");
    assert_eq!(p.cmd_args.first().map(String::as_str), Some("-c"));
}

#[cfg(unix)]
#[test]
fn run_command_timeout_includes_partial_output_and_kills() {
    use crate::cm_tools::subprocess_session::SubprocessWaitCtl;
    let marker = format!("cm_p0_rc_{}", std::process::id());
    let args = format!(
        r#"{{"command":"bash","args":["-c","echo partial-out; sleep 60 # {marker}"]}}"#
    );
    let err = run_checked_wait(
        &args,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
        &SubprocessWaitCtl::with_wall_secs(1),
    )
    .expect_err("timeout");
    assert_eq!(err.kind(), "timeout");
    let msg = err.user_message();
    assert!(msg.contains("命令执行超时"), "{msg}");
    assert!(msg.contains("partial-out"), "{msg}");
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(
        !crate::cm_tools::subprocess_session::proc_cmdline_contains(&marker),
        "sleep grandchild still running for {marker}"
    );
}

#[cfg(unix)]
#[test]
fn run_command_timeout_legacy_parsed_keeps_stdout() {
    use crate::cm_tools::subprocess_session::SubprocessWaitCtl;
    let err = run_checked_wait(
        r#"{"command":"bash","args":["-c","echo partial-out; sleep 60"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
        &SubprocessWaitCtl::with_wall_secs(1),
    )
    .expect_err("timeout");
    let te = err.into_tool_error();
    assert_eq!(te.code, "timeout");
    assert!(
        te.legacy_parsed.stdout.contains("partial-out"),
        "stdout={:?}",
        te.legacy_parsed.stdout
    );
}

#[cfg(unix)]
#[test]
fn run_command_cancel_sets_cancelled_code() {
    use crate::cm_tools::subprocess_session::SubprocessWaitCtl;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let cancel = Arc::new(AtomicBool::new(false));
    let wait = SubprocessWaitCtl {
        wall: Some(std::time::Duration::from_secs(30)),
        cancel: Some(Arc::clone(&cancel)),
        extra_stop: None,
        chunk_sink: None,
    };
    let handle = std::thread::spawn(move || {
        run_checked_wait(
            r#"{"command":"sleep","args":["60"]}"#,
            TEST_MAX_OUTPUT_LEN,
            &test_allowed(),
            test_work_dir(),
            None,
            false,
            &wait,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(150));
    cancel.store(true, Ordering::SeqCst);
    let err = handle.join().expect("join").expect_err("cancelled");
    assert_eq!(err.kind(), "cancelled");
    assert!(err.user_message().contains("命令已取消"));
}
