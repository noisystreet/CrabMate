//! 有限的 Linux 命令执行工具（白名单、工作目录限制、无 shell 注入）

use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::output_util;

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

/// 在指定工作目录下执行白名单内的 Linux 命令，不经过 shell，输出截断。
/// `allowed_commands` 为可执行命令名列表（小写）；`working_dir` 为命令的工作目录（已校验为存在目录）。
pub fn run(
    args_json: &str,
    max_output_len: usize,
    allowed_commands: &[String],
    working_dir: &Path,
) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let cmd_name = match args.get("command").and_then(|c| c.as_str()) {
        Some(s) => s.trim().to_lowercase(),
        None => return "错误：缺少 command 参数".to_string(),
    };
    if !allowed_commands.iter().any(|c| c == &cmd_name) {
        return format!(
            "不允许的命令：{}。允许的命令：{}",
            cmd_name,
            allowed_commands.join(", ")
        );
    }
    let cmd_args: Vec<String> = match args.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(_) => return "错误：args 必须是字符串数组".to_string(),
        None => vec![],
    };
    for a in &cmd_args {
        if !is_arg_safe(a) {
            return "错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）".to_string();
        }
    }
    // 简单的每秒调用限流，防止高频滥用命令执行
    if let Err(e) = check_rate_limit() {
        return e;
    }
    let output = match Command::new(&cmd_name)
        .args(&cmd_args)
        .current_dir(working_dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format_spawn_error(&cmd_name, working_dir, &e),
    };
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

fn check_rate_limit() -> Result<(), String> {
    let mut state = RATE_LIMIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_sec();
    if now != state.window_sec {
        state.window_sec = now;
        state.count = 0;
    }
    if state.count >= MAX_COMMANDS_PER_SEC {
        return Err(format!(
            "命令调用过于频繁：每秒最多允许 {} 次，请稍后再试",
            MAX_COMMANDS_PER_SEC
        ));
    }
    state.count += 1;
    Ok(())
}

/// 将底层 IO 错误转为对用户更友好的提示
fn format_spawn_error(cmd: &str, working_dir: &Path, e: &io::Error) -> String {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => format!(
            "错误：命令 \"{}\" 不存在或在当前环境中不可用（工作目录：{}）",
            cmd,
            working_dir.display()
        ),
        PermissionDenied => format!(
            "错误：没有权限执行命令 \"{}\"（请检查可执行权限或安全策略）",
            cmd
        ),
        _ => format!("错误：无法执行命令 \"{}\"（系统错误：{}）", cmd, e),
    }
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
        );
        assert!(out.starts_with("参数解析错误"));
    }

    #[test]
    fn test_run_missing_command() {
        let out = run(
            r#"{"args":[]}"#,
            TEST_MAX_OUTPUT_LEN,
            &test_allowed(),
            test_work_dir(),
        );
        assert_eq!(out, "错误：缺少 command 参数");
    }

    #[test]
    fn test_run_disallowed_command() {
        let out = run(
            r#"{"command":"rm","args":["-rf","/"]}"#,
            TEST_MAX_OUTPUT_LEN,
            &test_allowed(),
            test_work_dir(),
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
        );
        assert!(out.contains("参数不允许"));
    }
}
