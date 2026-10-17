//! `run_command` 结构化执行结果（[`CommandOutcome`]）与调用摘要（[`CommandDigest`]）。
//!
//! 进程启动后的所有结束状态（成功、非零退出、超时、取消）都收敛到 [`ExitStatusKind`]；
//! 启动前失败仍由 [`super::RunCommandError`] 表达。

use std::path::Path;

use crate::cm_tools::tool_result::ToolError;

use super::output_util;

/// 单次输出最多保留的行数（stdout / stderr 分别截断）。
pub(super) const MAX_OUTPUT_LINES: usize = 500;

/// 进程结束状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatusKind {
    Exited { code: i32 },
    TimedOut { timeout_secs: u64 },
    Cancelled,
}

impl ExitStatusKind {
    /// 文本渲染使用的退出码；超时 / 取消沿用历史值 `-1`。
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            ExitStatusKind::Exited { code } => code,
            ExitStatusKind::TimedOut { .. } | ExitStatusKind::Cancelled => -1,
        }
    }
}

/// `run_command` 的执行结果（进程已启动）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub invocation: String,
    pub status: ExitStatusKind,
    pub stdout: String,
    pub stderr: String,
    /// 缓存命中：正文已带缓存横幅，渲染时原样返回。
    cached_body: Option<String>,
}

impl CommandOutcome {
    #[must_use]
    pub fn new(
        invocation: String,
        status: ExitStatusKind,
        stdout: String,
        stderr: String,
    ) -> Self {
        Self {
            invocation,
            status,
            stdout,
            stderr,
            cached_body: None,
        }
    }

    /// 测试结果缓存命中：正文已含横幅，原样返回。
    #[must_use]
    pub fn cached(body: String) -> Self {
        Self {
            invocation: String::new(),
            status: ExitStatusKind::Exited { code: 0 },
            stdout: String::new(),
            stderr: String::new(),
            cached_body: Some(body),
        }
    }

    /// 渲染为模型可见文本；输出格式与历史实现逐字一致。
    #[must_use]
    pub fn render(&self, max_output_len: usize) -> String {
        if let Some(body) = &self.cached_body {
            return body.clone();
        }
        let exit_code = self.status.exit_code();
        let truncate =
            |s: &str| output_util::truncate_output_lines(s, max_output_len, MAX_OUTPUT_LINES);
        let mut out = format!("命令：{}\n退出码：{exit_code}\n", self.invocation);
        if !self.stdout.is_empty() {
            out.push_str("标准输出：\n");
            out.push_str(&truncate(&self.stdout));
        }
        if !self.stderr.is_empty() {
            out.push_str("标准错误：\n");
            out.push_str(&truncate(&self.stderr));
        }
        if self.stdout.is_empty() && self.stderr.is_empty() && exit_code == 0 {
            out.push_str("(无输出)");
        }
        let body = out.trim_end().to_string();
        match self.status {
            ExitStatusKind::Exited { .. } => body,
            ExitStatusKind::TimedOut { timeout_secs } => {
                format!("命令执行超时（{timeout_secs} 秒）；子进程已发送终止信号。\n{body}")
            }
            ExitStatusKind::Cancelled => {
                format!("命令已取消；子进程已发送终止信号。\n{body}")
            }
        }
    }

    /// 超时 / 取消 → 历史 `ToolError`（`timeout` / `cancelled`）；正常退出返回 `None`。
    #[must_use]
    pub fn session_stop_tool_error(&self, rendered: &str) -> Option<ToolError> {
        match self.status {
            ExitStatusKind::Exited { .. } => None,
            ExitStatusKind::TimedOut { .. } => {
                Some(ToolError::session_stop("timeout", rendered.to_string()))
            }
            ExitStatusKind::Cancelled => {
                Some(ToolError::session_stop("cancelled", rendered.to_string()))
            }
        }
    }
}

/// `run_command` 调用摘要：去重与测试结果缓存共用的稳定 key。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDigest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

impl CommandDigest {
    #[must_use]
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            cwd: None,
        }
    }

    /// 由调用形态构造（`shell` / `script` 已包含在 `command` / `args` 中，无需重复记录）。
    #[must_use]
    pub fn from_invocation(command: impl Into<String>, args: Vec<String>) -> Self {
        Self::new(command, args)
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: &Path) -> Self {
        self.cwd = Some(cwd.to_string_lossy().into_owned());
        self
    }

    /// 稳定 key；无 `cwd` 时保持历史形状，避免无谓的 key 迁移。
    #[must_use]
    pub fn key(&self) -> String {
        let base = format!("run_command|{}|{}", self.command, self.args.join("\u{1f}"));
        let Some(cwd) = &self.cwd else {
            return base;
        };
        format!(
            "{base}|meta={}",
            serde_json::json!({ "cwd": cwd })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandDigest, CommandOutcome, ExitStatusKind};
    use crate::cm_tools::tool_result::ToolFailureCategory;
    use std::path::Path;

    const MAX: usize = 10_000;

    #[test]
    fn render_success_formats_streams_and_trims_trailing_newline() {
        let outcome = CommandOutcome::new(
            "echo hi".to_string(),
            ExitStatusKind::Exited { code: 0 },
            "hi\n".to_string(),
            String::new(),
        );
        assert_eq!(
            outcome.render(MAX),
            "命令：echo hi\n退出码：0\n标准输出：\nhi"
        );
    }

    #[test]
    fn render_success_without_output_uses_placeholder() {
        let outcome = CommandOutcome::new(
            "true".to_string(),
            ExitStatusKind::Exited { code: 0 },
            String::new(),
            String::new(),
        );
        assert_eq!(outcome.render(MAX), "命令：true\n退出码：0\n(无输出)");
    }

    #[test]
    fn render_nonzero_exit_keeps_stderr_and_skips_placeholder() {
        let outcome = CommandOutcome::new(
            "false".to_string(),
            ExitStatusKind::Exited { code: 1 },
            String::new(),
            "boom\n".to_string(),
        );
        assert_eq!(
            outcome.render(MAX),
            "命令：false\n退出码：1\n标准错误：\nboom"
        );
    }

    #[test]
    fn render_timeout_prefix_and_exit_code_minus_one() {
        let outcome = CommandOutcome::new(
            "sleep 10".to_string(),
            ExitStatusKind::TimedOut { timeout_secs: 3 },
            "partial\n".to_string(),
            String::new(),
        );
        assert_eq!(
            outcome.render(MAX),
            "命令执行超时（3 秒）；子进程已发送终止信号。\n命令：sleep 10\n退出码：-1\n标准输出：\npartial"
        );
    }

    #[test]
    fn render_cancelled_prefix() {
        let outcome = CommandOutcome::new(
            "sleep 10".to_string(),
            ExitStatusKind::Cancelled,
            String::new(),
            String::new(),
        );
        assert_eq!(
            outcome.render(MAX),
            "命令已取消；子进程已发送终止信号。\n命令：sleep 10\n退出码：-1"
        );
    }

    #[test]
    fn render_cached_body_verbatim() {
        let outcome = CommandOutcome::cached("缓存命中：...\n".to_string());
        assert_eq!(outcome.render(MAX), "缓存命中：...\n");
    }

    #[test]
    fn session_stop_tool_error_maps_timeout_and_cancelled() {
        let timed_out = CommandOutcome::new(
            "sleep 10".to_string(),
            ExitStatusKind::TimedOut { timeout_secs: 1 },
            String::new(),
            String::new(),
        );
        let err = timed_out
            .session_stop_tool_error("rendered")
            .expect("timeout error");
        assert_eq!(err.code, "timeout");
        assert_eq!(err.category, ToolFailureCategory::Timeout);
        assert!(err.retryable);

        let cancelled = CommandOutcome::new(
            "sleep 10".to_string(),
            ExitStatusKind::Cancelled,
            String::new(),
            String::new(),
        );
        let err = cancelled
            .session_stop_tool_error("rendered")
            .expect("cancel error");
        assert_eq!(err.code, "cancelled");
        assert!(!err.retryable);

        let exited = CommandOutcome::new(
            "true".to_string(),
            ExitStatusKind::Exited { code: 0 },
            String::new(),
            String::new(),
        );
        assert!(exited.session_stop_tool_error("rendered").is_none());
    }

    #[test]
    fn digest_key_shape() {
        let base = CommandDigest::new("cmake".to_string(), vec!["--build".to_string(), "build".to_string()]);
        assert_eq!(base.key(), "run_command|cmake|--build\u{1f}build");

        let with_cwd = base.with_cwd(Path::new("/tmp/ws"));
        let key = with_cwd.key();
        assert!(key.starts_with("run_command|cmake|--build\u{1f}build|meta="), "{key}");
        assert!(key.contains(r#""cwd":"/tmp/ws""#), "{key}");
    }
}
