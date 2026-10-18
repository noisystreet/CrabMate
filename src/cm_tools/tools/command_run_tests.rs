use super::*;
use crate::cm_tools::subprocess_session::SubprocessWaitCtl;
use crate::cm_tools::tools::ExitStatusKind;
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
    "git",
];

fn test_allowed() -> Vec<String> {
    TEST_ALLOWED.iter().map(|s| s.to_string()).collect()
}

fn test_work_dir() -> &'static Path {
    Path::new(".")
}

/// 旧 6 参入口的测试壳：解析 JSON 后调用 [`super::run`]，避免测试依赖已弃用签名。
fn run(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> String {
    let wait = SubprocessWaitCtl::default();
    match parse_command_spec(args_json, working_dir) {
        Ok(spec) => match super::run(
            &spec,
            RunOptions {
                max_output_len,
                allowed_commands,
                working_dir,
                test_cache,
                skip_arg_safety,
                wait: &wait,
            },
        ) {
            Ok(outcome) => {
                let rendered = outcome.render(max_output_len);
                match outcome.session_stop_tool_error(&rendered) {
                    Some(e) => e.message,
                    None => rendered,
                }
            }
            Err(e) => e.extended_user_message(),
        },
        Err(e) => e.extended_user_message(),
    }
}

fn run_checked(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
) -> Result<CommandOutcome, RunCommandError> {
    let wait = SubprocessWaitCtl::default();
    let spec = parse_command_spec(args_json, working_dir)?;
    super::run(
        &spec,
        RunOptions {
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait: &wait,
        },
    )
}

fn run_checked_wait(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
    skip_arg_safety: bool,
    wait: &SubprocessWaitCtl,
) -> Result<CommandOutcome, RunCommandError> {
    let spec = parse_command_spec(args_json, working_dir)?;
    super::run(
        &spec,
        RunOptions {
            max_output_len,
            allowed_commands,
            working_dir,
            test_cache,
            skip_arg_safety,
            wait,
        },
    )
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
    let spec = parse_command_spec(
        r#"{"command":"cd","args":["src","&&","echo","peeled"]}"#,
        Path::new("."),
    )
    .expect("spec");
    let p = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false).expect("prep");
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
    let spec = parse_command_spec(r#"{"command":"cd","args":["src"]}"#, Path::new("."))
        .expect("spec");
    let e = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false)
        .err()
        .expect("cd alone");
    assert_eq!(e.kind(), "cd_prefix_invalid");
}

#[test]
fn prepare_splits_embedded_command_prefix() {
    let spec = parse_command_spec(
        r#"{"command":"pre-commit run --all-files","args":[]}"#,
        Path::new("."),
    )
    .expect("spec");
    let p = prepare_run_command_spec(&spec, Path::new("."), &["pre-commit".to_string()], false)
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

    let spec = parse_command_spec(r#"{"command":"./","args":["hello/build/hello"]}"#, dir.path())
        .expect("spec");
    let p = prepare_run_command_spec(&spec, dir.path(), &[], false).expect("prep");
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
fn embedded_bash_c_keeps_escaped_dollar_literal() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"bash -c echo \\$HOME"}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("$HOME"), "{out}");
}

#[test]
fn wrapped_shell_script_keeps_escaped_dollar_literal() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"echo","args":["a \\$HOME b"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("a $HOME b"), "{out}");
}

#[test]
fn backticks_in_commit_message_stay_literal() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"echo","args":["fix `id` in docs"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("fix `id` in docs"), "{out}");
}

#[test]
fn command_substitution_is_not_executed_when_wrapped() {
    let mut allowed = test_allowed();
    allowed.push("bash".into());
    let out = run(
        r#"{"command":"echo","args":["*.rs","fix `echo pwned`"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("fix `echo pwned`"), "{out}");
    let out = run(
        r#"{"command":"echo","args":["*.rs","fix $(echo pwned)"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &allowed,
        test_work_dir(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("fix $(echo pwned)"), "{out}");
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

#[test]
fn prepare_peels_multiple_cd_prefixes() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("a/b")).expect("mkdir");
    let spec = parse_command_spec(
        r#"{"command":"cd","args":["a","&&","cd","b","&&","echo","x"]}"#,
        dir.path(),
    )
    .expect("spec");
    let p = prepare_run_command_spec(&spec, dir.path(), &test_allowed(), false).expect("prep");
    assert_eq!(p.cmd_name, "echo");
    assert_eq!(p.cmd_args, vec!["x".to_string()]);
    assert!(
        p.effective_working_dir.ends_with("a/b"),
        "{:?}",
        p.effective_working_dir
    );
}

#[test]
fn prepare_allows_cmake_parent_dir_arg() {
    let spec = parse_command_spec(r#"{"command":"cmake","args":[".."]}"#, Path::new("."))
        .expect("spec");
    let p = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false)
        .expect("cmake .. must stay allowed");
    assert_eq!(p.cmd_name, "cmake");
    assert_eq!(p.cmd_args, vec!["..".to_string()]);
}

#[test]
fn prepare_allows_git_revision_range_arg() {
    let spec = parse_command_spec(r#"{"command":"git","args":["log","main..HEAD"]}"#, Path::new("."))
        .expect("spec");
    let p = prepare_run_command_spec(&spec, Path::new("."), &test_allowed(), false)
        .expect("git rev range must stay allowed");
    assert_eq!(p.cmd_name, "git");
    assert_eq!(
        p.cmd_args,
        vec!["log".to_string(), "main..HEAD".to_string()]
    );
}

#[test]
fn bash_on_allowlist_wraps_glob() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").expect("write");
    let out = run(
        r#"{"command":"ls","args":["*.rs"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        dir.path(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("命令：ls *.rs"), "{out}");
}

#[test]
fn bash_on_allowlist_wraps_pipe() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").expect("write");
    let out = run(
        r#"{"command":"ls","args":["|","wc","-l"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        dir.path(),
        None,
        false,
    );
    assert!(out.contains("退出码：0"), "{out}");
    assert!(out.contains("命令：ls | wc -l"), "{out}");
}

fn cargo_test_prepared(cwd: std::path::PathBuf) -> PreparedRunCommand {
    PreparedRunCommand {
        cmd_raw: "cargo".to_string(),
        cmd_name: "cargo".to_string(),
        exec_path: None,
        cmd_args: vec!["test".to_string()],
        effective_working_dir: cwd,
        inject_gh_token: false,
        rewrites: Vec::new(),
    }
}

fn cache_write_key(plan: RunCommandPlan) -> TestCacheKey {
    match plan {
        RunCommandPlan::Spawn(spawn) => spawn
            .cache_write
            .expect("eligible cargo test should carry cache_write")
            .key,
        RunCommandPlan::CacheHit { .. } => panic!("fresh key must not hit cache"),
    }
}

#[test]
fn cargo_test_cache_key_includes_effective_working_dir() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("a")).expect("mkdir a");
    std::fs::create_dir_all(root.path().join("b")).expect("mkdir b");
    let opts = RunCommandTestCacheOpts {
        enabled: true,
        max_entries: 8,
        workspace_root: root.path(),
    };
    let key_a = cache_write_key(
        plan_cargo_test_cache(
            &cargo_test_prepared(root.path().join("a")),
            Some(&opts),
            "cargo test".to_string(),
        )
        .expect("plan a"),
    );
    let key_b = cache_write_key(
        plan_cargo_test_cache(
            &cargo_test_prepared(root.path().join("b")),
            Some(&opts),
            "cargo test".to_string(),
        )
        .expect("plan b"),
    );
    assert_ne!(
        key_a, key_b,
        "cargo test cache key must distinguish working directory"
    );
}

#[cfg(unix)]
#[test]
fn run_command_timeout_includes_partial_output_and_kills() {
    use crate::cm_tools::subprocess_session::SubprocessWaitCtl;
    let marker = format!("cm_p0_rc_{}", std::process::id());
    let args = format!(
        r#"{{"command":"bash","args":["-c","echo partial-out; sleep 60 # {marker}"]}}"#
    );
    let outcome = run_checked_wait(
        &args,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
        &SubprocessWaitCtl::with_wall_secs(1),
    )
    .expect("timeout outcome");
    assert!(
        matches!(outcome.status, ExitStatusKind::TimedOut { .. }),
        "{:?}",
        outcome.status
    );
    let msg = outcome.render(TEST_MAX_OUTPUT_LEN);
    assert!(msg.contains("命令执行超时"), "{msg}");
    assert!(msg.contains("partial-out"), "{msg}");
    let te = outcome
        .session_stop_tool_error(&msg)
        .expect("timeout tool error");
    assert_eq!(te.code, "timeout");
    assert!(te.retryable);
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
    let outcome = run_checked_wait(
        r#"{"command":"bash","args":["-c","echo partial-out; sleep 60"]}"#,
        TEST_MAX_OUTPUT_LEN,
        &test_allowed(),
        test_work_dir(),
        None,
        false,
        &SubprocessWaitCtl::with_wall_secs(1),
    )
    .expect("timeout outcome");
    let rendered = outcome.render(TEST_MAX_OUTPUT_LEN);
    let te = outcome
        .session_stop_tool_error(&rendered)
        .expect("timeout tool error");
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
        uncapped_live: false,
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
    let outcome = handle.join().expect("join").expect("cancelled outcome");
    assert!(
        matches!(outcome.status, ExitStatusKind::Cancelled),
        "{:?}",
        outcome.status
    );
    let rendered = outcome.render(TEST_MAX_OUTPUT_LEN);
    let te = outcome
        .session_stop_tool_error(&rendered)
        .expect("cancelled tool error");
    assert_eq!(te.code, "cancelled");
    assert!(te.message.contains("命令已取消"), "{}", te.message);
}
