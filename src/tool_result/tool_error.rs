//! 工具失败时的结构化错误（与既有「整段 String + `parse_legacy_output`」并存，供编排与后续 runner 迁移）。
//!
//! 当前各 `tools::runner` 仍返回 `String`；[`super::parse_legacy_output`] 从正文推断 `ok` / `error_code`。
//! [`ToolError::from_parsed_legacy`] 将一次解析结果升为显式 `Err`，[`crate::tools::run_tool_try`] 在边界上返回 [`Result`]。

use std::fmt;

use super::{ParsedLegacyOutput, tool_error_retryable_heuristic};

/// 粗粒度失败分类（便于指标、重试策略与日志聚合；**不**保证与上游 HTTP 码一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)] // `Internal` 等：预留给 runner 显式返回的内部失败，当前仍多由 legacy 字符串推断
pub enum ToolFailureCategory {
    /// 参数 JSON、必填字段、类型不符等
    InvalidInput,
    /// 白名单、路径策略、审批拒绝等
    PolicyDenied,
    /// 工作区未设置、路径不在允许根内等
    Workspace,
    /// 工具或子进程超时
    Timeout,
    /// 外部命令非零退出、HTTP 业务失败等
    External,
    /// 工具内部逻辑错误（不含上游进程输出）
    Internal,
    /// 无法归类或未知工具名
    Unknown,
}

impl ToolFailureCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::PolicyDenied => "policy_denied",
            Self::Workspace => "workspace",
            Self::Timeout => "timeout",
            Self::External => "external",
            Self::Internal => "internal",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ToolFailureCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 工具执行失败（显式 `Err` 分支）；成功路径仍用 `String` 或后续 `Ok(String)`。
#[derive(Debug, Clone)]
pub struct ToolError {
    pub category: ToolFailureCategory,
    /// 与 [`super::ToolResult::error_code`] / 信封 `error_code` 对齐的短码
    pub code: String,
    /// 完整工具输出正文（与既有 `run_tool` 返回值一致，可直接写回 `role: tool`）
    pub message: String,
    /// 与 [`super::tool_error_retryable_heuristic`] 一致；编排/信封侧可读取（库内主路径尚未消费该字段）。
    #[allow(dead_code)]
    pub retryable: bool,
    /// 与 `message` 对应的 [`super::parse_legacy_output`] 结果（供 `run_tool_result` 等失败路径免再解析）。
    pub(crate) legacy_parsed: ParsedLegacyOutput,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let head = self.message.lines().next().unwrap_or("").trim();
        if head.is_empty() {
            write!(f, "[{}] {}", self.category.as_str(), self.code)
        } else {
            write!(f, "[{}][{}] {}", self.category.as_str(), self.code, head)
        }
    }
}

impl std::error::Error for ToolError {}

fn category_for_error_code(code: &str) -> ToolFailureCategory {
    match code {
        "invalid_args" | "missing_command" => ToolFailureCategory::InvalidInput,
        "unknown_tool" => ToolFailureCategory::Unknown,
        "command_not_allowed" => ToolFailureCategory::PolicyDenied,
        "workspace_not_set" | "workspace_no_cargo_toml" | "codebase_semantic_unconfigured" => {
            ToolFailureCategory::Workspace
        }
        "timeout" => ToolFailureCategory::Timeout,
        "rate_limited" => ToolFailureCategory::PolicyDenied,
        "command_not_found" | "permission_denied" | "spawn_failed" | "cargo_spawn_failed" => {
            ToolFailureCategory::External
        }
        "read_file_invalid_range" => ToolFailureCategory::InvalidInput,
        "read_file_not_file"
        | "read_file_io"
        | "read_file_utf8_decode"
        | "read_file_encoding"
        | "read_file_internal" => ToolFailureCategory::External,
        c if c.starts_with("read_file_workspace_") => ToolFailureCategory::Workspace,
        "search_in_files_invalid_regex" | "search_in_files_invalid_glob" => {
            ToolFailureCategory::InvalidInput
        }
        "search_in_files_workspace_base_resolve_failed"
        | "search_in_files_workspace_subpath_resolve_failed"
        | "search_in_files_workspace_outside_root"
        | "search_in_files_path_absolute_not_allowed" => ToolFailureCategory::Workspace,
        c if c.ends_with("_failed") => ToolFailureCategory::External,
        _ => ToolFailureCategory::Unknown,
    }
}

impl ToolError {
    /// 由 [`super::parse_legacy_output`] 的结果构造（`raw_output` 为完整 `run_tool` 字符串）。
    pub fn from_parsed_legacy(
        tool_name: &str,
        parsed: &ParsedLegacyOutput,
        raw_output: String,
    ) -> Self {
        let code = parsed
            .error_code
            .clone()
            .unwrap_or_else(|| format!("{tool_name}_failed"));
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        let category = category_for_error_code(code.as_str());
        Self {
            category,
            code,
            message: raw_output,
            retryable,
            legacy_parsed: parsed.clone(),
        }
    }

    /// 参数 / JSON 校验失败（`error_code`：`invalid_args`）。
    pub fn invalid_args(message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some("invalid_args".to_string()),
        };
        Self {
            category: ToolFailureCategory::InvalidInput,
            code: "invalid_args".to_string(),
            message,
            retryable: false,
            legacy_parsed: parsed,
        }
    }

    /// 工作区布局或前置条件问题（如缺少 `Cargo.toml`）；`code` 须与 [`category_for_error_code`] 一致。
    pub fn workspace(code: &'static str, message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.to_string()),
        };
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        Self {
            category: category_for_error_code(code),
            code: code.to_string(),
            message,
            retryable,
            legacy_parsed: parsed,
        }
    }

    /// 未知工具名（与历史「未知工具：…」正文一致）。
    pub fn unknown_tool(name: &str) -> Self {
        let message = format!("未知工具：{}", name);
        let parsed = super::parse_legacy_output(name, &message);
        Self::from_parsed_legacy(name, &parsed, message)
    }

    /// `run_command` 每秒限流等业务策略（`error_code`：`rate_limited`）。
    pub fn rate_limited(message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some("rate_limited".to_string()),
        };
        Self {
            category: ToolFailureCategory::PolicyDenied,
            code: "rate_limited".to_string(),
            message,
            retryable: true,
            legacy_parsed: parsed,
        }
    }

    /// 白名单拒绝等（`error_code`：`command_not_allowed`）；正文须使 [`super::parse_legacy_output`] 能识别为失败。
    pub fn command_not_allowed(message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some("command_not_allowed".to_string()),
        };
        Self {
            category: ToolFailureCategory::PolicyDenied,
            code: "command_not_allowed".to_string(),
            message,
            retryable: false,
            legacy_parsed: parsed,
        }
    }

    /// `cargo_*` 等子进程非零退出：`error_code` 为 `{tool_code}_failed`（如 `cargo_check_failed`）。
    pub fn cargo_subcommand_failed(tool_code: &str, exit_code: i32, message: String) -> Self {
        let code = format!("{tool_code}_failed");
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: Some(exit_code),
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.clone()),
        };
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        Self {
            category: ToolFailureCategory::External,
            code,
            message,
            retryable,
            legacy_parsed: parsed,
        }
    }

    /// 无法启动子进程（如找不到 `cargo` 可执行文件）。
    pub fn subprocess_spawn_error(title: &str, err: std::io::Error) -> Self {
        let message = format!("{}: 执行失败（{}）", title, err);
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some("cargo_spawn_failed".to_string()),
        };
        Self {
            category: ToolFailureCategory::External,
            code: "cargo_spawn_failed".to_string(),
            message,
            retryable: false,
            legacy_parsed: parsed,
        }
    }

    /// 外部工具/IO 类失败（`error_code` 由调用方指定，须与 [`category_for_error_code`] 一致）。
    pub fn external_code(code: &'static str, message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.to_string()),
        };
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        Self {
            category: category_for_error_code(code),
            code: code.to_string(),
            message,
            retryable,
            legacy_parsed: parsed,
        }
    }

    /// 逻辑或内部不变量破坏（极少见）；`error_code` 建议 `*_internal`。
    pub fn internal_code(code: &'static str, message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.to_string()),
        };
        Self {
            category: ToolFailureCategory::Internal,
            code: code.to_string(),
            message,
            retryable: false,
            legacy_parsed: parsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_result::parse_legacy_output;

    #[test]
    fn tool_error_from_unknown_tool_legacy() {
        let raw = "未知工具：foo".to_string();
        let p = parse_legacy_output("foo", &raw);
        assert!(!p.ok);
        let e = ToolError::from_parsed_legacy("foo", &p, raw.clone());
        assert_eq!(e.code, "unknown_tool");
        assert_eq!(e.category, ToolFailureCategory::Unknown);
        assert!(!e.retryable);
        assert_eq!(e.message, raw);
    }

    #[test]
    fn tool_error_timeout_retryable() {
        let raw = "错误：超时\n".to_string();
        let p = parse_legacy_output("run_command", &raw);
        let e = ToolError::from_parsed_legacy("run_command", &p, raw);
        assert_eq!(e.code, "timeout");
        assert!(e.retryable);
        assert_eq!(e.category, ToolFailureCategory::Timeout);
    }
}
