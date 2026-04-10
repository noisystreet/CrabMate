//! 有限的 Linux 命令执行工具（白名单、工作目录限制、无 shell 注入）

use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use super::output_util;
use super::test_result_cache::{
    TestCacheKey, TestCacheKind, cargo_test_run_command_args_fingerprint,
    fingerprint_rust_workspace_sources, store_cached, try_get_cached, wrap_cache_hit,
};
use crate::tool_result::{ParsedLegacyOutput, ToolError, ToolFailureCategory};

/// `run_command` 在参数校验、限流、启动进程前的失败原因（可判别；成功路径仍返回带退出码的 `String` 正文）。
#[derive(Debug, Error)]
pub enum RunCommandError {
    #[error("参数解析错误：{0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("错误：缺少 command 参数")]
    MissingCommand,
    #[error("不允许的命令：{attempted}。允许的命令：{allowed}")]
    DisallowedCommand { attempted: String, allowed: String },
    #[error("错误：args 必须是字符串数组")]
    ArgsNotArray,
    #[error("错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）")]
    UnsafeArg,
    #[error("命令调用过于频繁：每秒最多允许 {max_per_sec} 次，请稍后再试")]
    RateLimited { max_per_sec: u32 },
    #[error("错误：命令 \"{cmd}\" 不存在或在当前环境中不可用（工作目录：{work_dir}）")]
    CommandNotFound {
        cmd: String,
        work_dir: String,
        #[source]
        source: io::Error,
    },
    #[error("错误：没有权限执行命令 \"{cmd}\"（请检查可执行权限或安全策略）")]
    PermissionDenied {
        cmd: String,
        #[source]
        source: io::Error,
    },
    #[error("错误：无法执行命令 \"{cmd}\"（系统错误：{source}）")]
    SpawnOther {
        cmd: String,
        #[source]
        source: io::Error,
    },
}

impl RunCommandError {
    /// 转为 [`ToolError`]，供 `run_command` runner 显式返回（与信封 `error_code` 对齐）。
    #[must_use]
    pub fn into_tool_error(self) -> ToolError {
        let msg = match &self {
            RunCommandError::CommandNotFound { .. } => self.extended_user_message(),
            _ => self.user_message(),
        };
        match self {
            RunCommandError::JsonParse(_) => ToolError::invalid_args(msg),
            RunCommandError::MissingCommand => ToolError {
                category: ToolFailureCategory::InvalidInput,
                code: "missing_command".to_string(),
                message: msg,
                retryable: false,
                legacy_parsed: ParsedLegacyOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: Some("missing_command".to_string()),
                },
            },
            RunCommandError::DisallowedCommand { .. } => ToolError::command_not_allowed(msg),
            RunCommandError::ArgsNotArray | RunCommandError::UnsafeArg => {
                ToolError::invalid_args(msg)
            }
            RunCommandError::RateLimited { .. } => ToolError::rate_limited(msg),
            RunCommandError::CommandNotFound { .. } => ToolError {
                category: ToolFailureCategory::External,
                code: "command_not_found".to_string(),
                message: msg,
                retryable: false,
                legacy_parsed: ParsedLegacyOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: Some("command_not_found".to_string()),
                },
            },
            RunCommandError::PermissionDenied { .. } => ToolError {
                category: ToolFailureCategory::External,
                code: "permission_denied".to_string(),
                message: msg,
                retryable: false,
                legacy_parsed: ParsedLegacyOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: Some("permission_denied".to_string()),
                },
            },
            RunCommandError::SpawnOther { .. } => ToolError {
                category: ToolFailureCategory::External,
                code: "spawn_failed".to_string(),
                message: msg,
                retryable: false,
                legacy_parsed: ParsedLegacyOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: Some("spawn_failed".to_string()),
                },
            },
        }
    }

    /// 简短分类键，供 metrics / 结构化日志（不含命令名等细节时可只记此项）。
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            RunCommandError::JsonParse(_) => "json_parse",
            RunCommandError::MissingCommand => "missing_command",
            RunCommandError::DisallowedCommand { .. } => "disallowed_command",
            RunCommandError::ArgsNotArray => "args_not_array",
            RunCommandError::UnsafeArg => "unsafe_arg",
            RunCommandError::RateLimited { .. } => "rate_limited",
            RunCommandError::CommandNotFound { .. } => "command_not_found",
            RunCommandError::PermissionDenied { .. } => "permission_denied",
            RunCommandError::SpawnOther { .. } => "spawn_other",
        }
    }

    /// 与历史工具输出一致的完整说明。
    #[must_use]
    pub fn user_message(&self) -> String {
        self.to_string()
    }

    /// 与 [`user_message`] 相同；若为本变体为 [`RunCommandError::CommandNotFound`] 且命中内置表，文末追加 CLI 安装提示。
    #[must_use]
    pub fn extended_user_message(&self) -> String {
        let mut s = self.user_message();
        if let RunCommandError::CommandNotFound { cmd, .. } = self
            && let Some(h) = output_util::cli_missing_install_hint(cmd)
        {
            s.push_str("\n\n");
            s.push_str(h);
        }
        s
    }
}

fn map_spawn_error(cmd: &str, working_dir: &Path, e: io::Error) -> RunCommandError {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => RunCommandError::CommandNotFound {
            cmd: cmd.to_string(),
            work_dir: working_dir.display().to_string(),
            source: e,
        },
        PermissionDenied => RunCommandError::PermissionDenied {
            cmd: cmd.to_string(),
            source: e,
        },
        _ => RunCommandError::SpawnOther {
            cmd: cmd.to_string(),
            source: e,
        },
    }
}

/// 简单的每秒调用限流器状态
struct RateLimitState {
    window_sec: u64,
    count: u32,
}

/// 全局限流器：在任意 1 秒窗口内最多允许执行的命令数
const MAX_COMMANDS_PER_SEC: u32 = 5;

static RATE_LIMIT: Mutex<RateLimitState> = Mutex::new(RateLimitState {
    window_sec: 0,
    count: 0,
});

const MAX_OUTPUT_LINES: usize = 500;

fn is_arg_safe(arg: &str) -> bool {
    let a = arg.trim();
    !a.contains("..") && !a.starts_with('/')
}

/// 可选：`cargo test …` 的进程内结果缓存（与内置 `cargo_test` 工具共用指纹逻辑）。
pub(crate) struct RunCommandTestCacheOpts<'a> {
    pub enabled: bool,
    pub max_entries: usize,
    pub workspace_root: &'a Path,
}

fn cargo_test_argv_cache_eligible(cmd_args: &[String]) -> bool {
    if cmd_args.first().map(|s| s.as_str()) != Some("test") {
        return false;
    }
    for a in cmd_args {
        if a == "--nocapture" || a == "--test-threads" {
            return false;
        }
    }
    true
}

/// 在指定工作目录下执行白名单内的 Linux 命令，不经过 shell，输出截断。
/// `allowed_commands` 为可执行命令名列表（小写）；`working_dir` 为命令的工作目录（已校验为存在目录）。
pub fn run(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
) -> String {
    run_impl(
        args_json,
        max_output_len,
        allowed_commands,
        working_dir,
        test_cache,
    )
    .unwrap_or_else(|e| e.extended_user_message())
}

/// 与 [`run`] 相同，失败时返回 [`ToolError`]（显式 `error_code` / 分类，不经字符串启发式）。
#[allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 `run_tool_dispatch` 一致
pub fn run_try(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
) -> Result<String, ToolError> {
    run_impl(
        args_json,
        max_output_len,
        allowed_commands,
        working_dir,
        test_cache,
    )
    .map_err(RunCommandError::into_tool_error)
}

/// 与 [`run`] 相同，失败时返回结构化错误（成功仍为格式化输出字符串）。
pub(crate) fn run_checked(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
) -> Result<String, RunCommandError> {
    run_impl(
        args_json,
        max_output_len,
        allowed_commands,
        working_dir,
        test_cache,
    )
}

fn run_impl(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
    test_cache: Option<RunCommandTestCacheOpts<'_>>,
) -> Result<String, RunCommandError> {
    let args: serde_json::Value = serde_json::from_str(args_json)?;
    let cmd_name = match args.get("command").and_then(|c| c.as_str()) {
        Some(s) => s.trim().to_lowercase(),
        None => return Err(RunCommandError::MissingCommand),
    };
    if !allowed_commands.iter().any(|c| c == &cmd_name) {
        return Err(RunCommandError::DisallowedCommand {
            attempted: cmd_name,
            allowed: allowed_commands.join(", "),
        });
    }
    let cmd_args: Vec<String> = match args.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => return Err(RunCommandError::ArgsNotArray),
        None => vec![],
    };
    for a in &cmd_args {
        if !is_arg_safe(a) {
            return Err(RunCommandError::UnsafeArg);
        }
    }
    check_rate_limit()?;

    if cmd_name == "cargo"
        && let Some(opts) = test_cache.as_ref()
        && opts.enabled
        && cargo_test_argv_cache_eligible(&cmd_args)
        && let Some(inputs_fp) = fingerprint_rust_workspace_sources(opts.workspace_root)
    {
        let args_fp = cargo_test_run_command_args_fingerprint(&cmd_args);
        let key = TestCacheKey {
            workspace_root: opts.workspace_root.to_path_buf(),
            kind: TestCacheKind::CargoTestViaRunCommand,
            args_fingerprint: args_fp,
            inputs_fingerprint: inputs_fp.clone(),
        };
        if let Some(hit) = try_get_cached(opts.enabled, opts.max_entries, &key) {
            return Ok(wrap_cache_hit(&inputs_fp, &hit));
        }
        let output = Command::new(&cmd_name)
            .args(&cmd_args)
            .current_dir(working_dir)
            .output()
            .map_err(|e| map_spawn_error(&cmd_name, working_dir, e))?;
        let formatted = format_command_output(&cmd_name, output, max_output_len);
        store_cached(opts.enabled, opts.max_entries, key, formatted.clone());
        return Ok(formatted);
    }

    let output = Command::new(&cmd_name)
        .args(&cmd_args)
        .current_dir(working_dir)
        .output()
        .map_err(|e| map_spawn_error(&cmd_name, working_dir, e))?;
    Ok(format_command_output(&cmd_name, output, max_output_len))
}

fn format_command_output(
    _cmd_name: &str,
    output: std::process::Output,
    max_output_len: usize,
) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let truncate =
        |s: &str| output_util::truncate_output_lines(s, max_output_len, MAX_OUTPUT_LINES);
    let status = output.status;
    let mut out = format!("退出码：{}\n", status.code().unwrap_or(-1));
    if !stdout.is_empty() {
        out.push_str("标准输出：\n");
        out.push_str(&truncate(&stdout));
    }
    if !stderr.is_empty() {
        out.push_str("标准错误：\n");
        out.push_str(&truncate(&stderr));
    }
    if stdout.is_empty() && stderr.is_empty() && status.success() {
        out.push_str("(无输出)");
    }
    out.trim_end().to_string()
}

fn now_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn check_rate_limit() -> Result<(), RunCommandError> {
    let mut state = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_sec();
    if now != state.window_sec {
        state.window_sec = now;
        state.count = 0;
    }
    if state.count >= MAX_COMMANDS_PER_SEC {
        return Err(RunCommandError::RateLimited {
            max_per_sec: MAX_COMMANDS_PER_SEC,
        });
    }
    state.count += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
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
        );
        assert!(out.contains("参数不允许"));
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
}
