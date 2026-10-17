//! `run_command` 的错误类型与分类映射（与执行细节解耦）。

use std::io;
use std::path::Path;

use thiserror::Error;

use super::output_util;
use crate::cm_tools::tool_result::{
    ParsedLegacyOutput, ToolError, ToolFailureCategory, parse_legacy_output,
};

use super::command_line_prepare::CdPeelError;

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
    #[error("错误：参数不允许包含父目录穿越（如 ..、../）或绝对路径（以 / 开头）")]
    UnsafeArg,
    /// 参数含 Shell 展开（`$(…)` / `$VAR` / glob 等），且白名单无 `bash`/`sh`、也未经脚本审批。
    #[error(
        "错误：参数含 Shell 展开 `{pattern}`。请将 bash 或 sh 加入 allowed_commands，或经 Web 审批后以 bash -c 执行整行脚本"
    )]
    ShellVariableDetected { pattern: String },
    /// `cd` 在无 shell 下不可直接 `exec`；仅支持前缀 `cd <相对目录> && <命令…>`（可多次串联），见 `command_line_prepare::peel_workspace_cd_prefix`.
    #[error("错误：`cd` 前缀无效：{detail}（当前工作目录：{work_dir}）")]
    CdPrefixInvalid { detail: String, work_dir: String },
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
    /// 墙钟超时；`formatted` 含截断后的部分 stdout/stderr。
    #[error("命令执行超时（{timeout_secs} 秒）")]
    TimedOut {
        timeout_secs: u64,
        formatted: String,
    },
    /// 用户取消或 SSE 关闭；`formatted` 含截断后的部分输出。
    #[error("命令已取消")]
    Cancelled { formatted: String },
}

impl RunCommandError {
    /// 转为 [`ToolError`]，供 `run_command` runner 显式返回（与信封 `error_code` 对齐）。
    #[must_use]
    pub fn into_tool_error(self) -> ToolError {
        let msg = match &self {
            RunCommandError::CommandNotFound { .. }
            | RunCommandError::TimedOut { .. }
            | RunCommandError::Cancelled { .. } => self.extended_user_message(),
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
            RunCommandError::DisallowedCommand { .. } => ToolError::approval_required(msg),
            RunCommandError::ArgsNotArray
            | RunCommandError::UnsafeArg
            | RunCommandError::CdPrefixInvalid { .. } => ToolError::invalid_args(msg),
            RunCommandError::ShellVariableDetected { .. } => ToolError {
                category: ToolFailureCategory::InvalidInput,
                code: "shell_variable_detected".to_string(),
                message: msg,
                retryable: true,
                legacy_parsed: ParsedLegacyOutput {
                    ok: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error_code: Some("shell_variable_detected".to_string()),
                },
            },
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
            RunCommandError::TimedOut { formatted, .. } => {
                session_stop_tool_error(formatted, "timeout", true)
            }
            RunCommandError::Cancelled { formatted } => {
                session_stop_tool_error(formatted, "cancelled", false)
            }
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
            RunCommandError::CdPrefixInvalid { .. } => "cd_prefix_invalid",
            RunCommandError::RateLimited { .. } => "rate_limited",
            RunCommandError::CommandNotFound { .. } => "command_not_found",
            RunCommandError::PermissionDenied { .. } => "permission_denied",
            RunCommandError::SpawnOther { .. } => "spawn_other",
            RunCommandError::ShellVariableDetected { .. } => "shell_variable_detected",
            RunCommandError::TimedOut { .. } => "timeout",
            RunCommandError::Cancelled { .. } => "cancelled",
        }
    }

    /// 与历史工具输出一致的完整说明。
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            RunCommandError::TimedOut { formatted, .. }
            | RunCommandError::Cancelled { formatted } => formatted.clone(),
            _ => self.to_string(),
        }
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

fn session_stop_tool_error(formatted: String, code: &str, retryable: bool) -> ToolError {
    let mut parsed = parse_legacy_output("run_command", &formatted);
    parsed.ok = false;
    parsed.error_code = Some(code.to_string());
    ToolError {
        category: ToolFailureCategory::Timeout,
        code: code.to_string(),
        message: formatted,
        retryable,
        legacy_parsed: parsed,
    }
}

impl From<CdPeelError> for RunCommandError {
    fn from(e: CdPeelError) -> Self {
        match e {
            CdPeelError::CdPrefixInvalid { detail, work_dir } => {
                RunCommandError::CdPrefixInvalid { detail, work_dir }
            }
            CdPeelError::UnsafeArg => RunCommandError::UnsafeArg,
            CdPeelError::SpawnOther { cmd, source } => RunCommandError::SpawnOther { cmd, source },
        }
    }
}

pub(super) fn map_spawn_error(cmd: &str, working_dir: &Path, e: io::Error) -> RunCommandError {
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
