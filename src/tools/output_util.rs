//! 工具输出截断公共函数。
//!
//! 多数工具在执行外部命令后需要对 stdout/stderr 做行数 + 字节数双重截断，
//! 避免超长输出占满上下文窗口。此模块统一实现，消除各工具文件中的重复 helper。
//!
//! 另含 **`Command::output()`** 后合并流、拼 **`title (exit=…):`** 块的共用逻辑（原分散在
//! `jvm_tools` / `go_tools` / `cargo_tools` 等多处）。

use std::process::Output;

/// 合并 **stdout / stderr** 的策略（不同 CLI 习惯不同）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessOutputMerge {
    /// 先 stdout，非空则换行再接 stderr（Maven、Go、`cargo` 子进程等多数工具）。
    ConcatStdoutStderr,
    /// 优先 stderr，否则 stdout（`tsc`、`eslint`、部分 Python / ast-grep 等）。
    StderrElseStdout,
}

/// 子进程**无法启动**（`spawn`/`output` 返回 `Err`）时的用户可见前缀风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandSpawnErrorStyle {
    /// `title: 无法启动命令（reason）`
    CannotStartCommand,
    /// `title: 无法启动（reason）。请确认已安装对应 CLI 且在 PATH 中。`
    CannotStartWithPathHint,
    /// `title: 执行失败（reason）`（与历史 `git` / `security_tools` 文案一致）
    ExecuteFailed,
}

/// 将子进程输出按策略合并为一段正文；若均为空则 **`(无输出)`**。
#[must_use]
pub(crate) fn merge_process_output(output: &Output, merge: ProcessOutputMerge) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let body = match merge {
        ProcessOutputMerge::ConcatStdoutStderr => {
            let mut body = String::new();
            if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(stderr.trim_end());
            }
            body
        }
        ProcessOutputMerge::StderrElseStdout => {
            if !stderr.trim().is_empty() {
                stderr.trim_end().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim_end().to_string()
            } else {
                String::new()
            }
        }
    };
    if body.trim().is_empty() {
        "(无输出)".to_string()
    } else {
        body
    }
}

/// `title (exit=code):\n` + 对 `body` 做行/字节截断（`body` 已含 **`(无输出)`** 时同样截断）。
#[must_use]
pub(crate) fn format_exited_command_output(
    title: &str,
    exit_code: i32,
    body: &str,
    max_bytes: usize,
    max_lines: usize,
) -> String {
    format!(
        "{} (exit={}):\n{}",
        title,
        exit_code,
        truncate_output_lines(body, max_bytes, max_lines)
    )
}

#[must_use]
pub(crate) fn format_spawn_error(
    title: &str,
    err: &std::io::Error,
    style: CommandSpawnErrorStyle,
) -> String {
    match style {
        CommandSpawnErrorStyle::CannotStartCommand => {
            format!("{title}: 无法启动命令（{err}）")
        }
        CommandSpawnErrorStyle::CannotStartWithPathHint => {
            format!("{title}: 无法启动（{err}）。请确认已安装对应 CLI 且在 PATH 中。")
        }
        CommandSpawnErrorStyle::ExecuteFailed => {
            format!("{title}: 执行失败（{err}）")
        }
    }
}

/// 执行 **`cmd.output()`**，合并输出并格式化为工具返回字符串；启动失败时按 **`spawn_style`** 生成说明。
#[must_use]
pub(crate) fn run_command_output_formatted(
    mut cmd: std::process::Command,
    title: &str,
    max_bytes: usize,
    max_lines: usize,
    merge: ProcessOutputMerge,
    spawn_style: CommandSpawnErrorStyle,
) -> String {
    match cmd.output() {
        Ok(output) => {
            let code = output.status.code().unwrap_or(-1);
            let body = merge_process_output(&output, merge);
            format_exited_command_output(title, code, &body, max_bytes, max_lines)
        }
        Err(e) => format_spawn_error(title, &e, spawn_style),
    }
}

/// UTF-8 安全的字节截断：在 `max_bytes` 以内找到最近的 char boundary 并截取。
pub(crate) fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// 行数 + 字节数双重截断（UTF-8 安全）。
///
/// 先按 `max_lines` 裁行，再按 `max_bytes` 裁字节。若发生截断则追加摘要后缀。
pub(super) fn truncate_output_lines(s: &str, max_bytes: usize, max_lines: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines && s.len() <= max_bytes {
        return s.to_string();
    }
    let kept_lines = lines.len().min(max_lines);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        truncate_to_char_boundary(&joined, max_bytes)
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}

/// 纯字节截断（UTF-8 安全），适用于不需要按行裁剪的场景（如 diff、结构化数据）。
pub(super) fn truncate_output_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let truncated = truncate_to_char_boundary(s, max_bytes);
    format!(
        "{}\n\n[输出已截断：共 {} 字节，上限 {} 字节]",
        truncated,
        s.len(),
        max_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_truncation_when_within_limits() {
        let s = "line1\nline2\nline3";
        assert_eq!(truncate_output_lines(s, 1000, 10), s);
    }

    #[test]
    fn truncate_by_line_count() {
        let s = "a\nb\nc\nd\ne";
        let out = truncate_output_lines(s, 10000, 3);
        assert!(out.starts_with("a\nb\nc\n"));
        assert!(out.contains("保留前 3 行"));
        assert!(out.contains("共 5 行"));
    }

    #[test]
    fn truncate_by_byte_limit() {
        let s = "x".repeat(200);
        let out = truncate_output_lines(&s, 50, 1000);
        assert!(out.contains("输出已截断"));
        assert!(out.len() < 200);
    }

    #[test]
    fn char_boundary_safety() {
        let s = "你好世界测试";
        let out = truncate_to_char_boundary(s, 7);
        assert!(out.len() <= 7);
        assert!(out == "你好");
    }

    #[test]
    fn truncate_bytes_only() {
        let s = "a".repeat(200);
        let out = truncate_output_bytes(&s, 50);
        assert!(out.contains("输出已截断"));
        assert!(out.contains("共 200 字节"));
    }

    #[test]
    fn truncate_bytes_no_op() {
        let s = "short";
        assert_eq!(truncate_output_bytes(s, 1000), s);
    }

    #[test]
    fn merge_concat_stdout_stderr() {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg("printf 'a\\n'; printf 'b' >&2")
            .output()
            .expect("sh");
        let m = merge_process_output(&out, ProcessOutputMerge::ConcatStdoutStderr);
        assert_eq!(m, "a\nb");
    }

    #[test]
    fn merge_stderr_else_stdout_prefers_stderr() {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg("printf 'out'; printf 'err' >&2")
            .output()
            .expect("sh");
        let m = merge_process_output(&out, ProcessOutputMerge::StderrElseStdout);
        assert_eq!(m, "err");
    }

    #[test]
    fn merge_empty_is_placeholder() {
        let out = std::process::Command::new("true").output().expect("true");
        assert_eq!(
            merge_process_output(&out, ProcessOutputMerge::ConcatStdoutStderr),
            "(无输出)"
        );
    }
}
