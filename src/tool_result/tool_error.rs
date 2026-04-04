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
        "invalid_args" => ToolFailureCategory::InvalidInput,
        "unknown_tool" => ToolFailureCategory::Unknown,
        "command_not_allowed" => ToolFailureCategory::PolicyDenied,
        "workspace_not_set" => ToolFailureCategory::Workspace,
        "timeout" => ToolFailureCategory::Timeout,
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
