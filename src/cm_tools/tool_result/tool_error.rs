//! 工具失败时的结构化错误（与既有「整段 String + `parse_legacy_output`」并存，供编排与后续 runner 迁移）。
//!
//! 当前各 `tools::runner` 仍返回 `String`；[`super::parse_legacy_output`] 从正文推断 `ok` / `error_code`。
//! [`ToolError::from_parsed_legacy`] 将一次解析结果升为显式 `Err`，[`crate::cm_tools::tools::run_tool_try`] 在边界上返回 [`Result`]。

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
    pub legacy_parsed: ParsedLegacyOutput,
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

/// 由 `error_code` 推导粗粒度 **`failure_category`**（与 SSE / `crabmate_tool` 同源；启发式，新增码可能落入 `unknown`）。
pub fn failure_category_for_error_code(code: &str) -> ToolFailureCategory {
    match code {
        "invalid_args" | "json_parse" | "args_not_array" | "missing_command" | "unsafe_arg"
        | "cd_prefix_invalid" => ToolFailureCategory::InvalidInput,
        "unknown_tool" => ToolFailureCategory::Unknown,
        "command_not_allowed"
        | "command_denied"
        | "approval_denied"
        | "approval_required"
        | "repeated_tool_failure_short_circuit"
        | "repeated_tool_family_failure_short_circuit" => ToolFailureCategory::PolicyDenied,
        "workflow_semaphore_closed"
        | "workflow_node_missing_result"
        | "workflow_tool_join_error" => ToolFailureCategory::External,
        // 「工作区不具备该工具的前置条件」：未设置工作区 / 无 `Cargo.toml` / 工作目录不是 Git 仓库
        // （`git` 族的 `rev-parse --is-inside-work-tree` 不成立）——换参数无用，须换目录。
        "workspace_not_set"
        | "workspace_no_cargo_toml"
        | "codebase_semantic_unconfigured"
        | "git_not_a_repo" => ToolFailureCategory::Workspace,
        "timeout" => ToolFailureCategory::Timeout,
        "cancelled" => ToolFailureCategory::Timeout,
        "http_timeout" => ToolFailureCategory::Timeout,
        "http_network_error" | "http_body_read_error" => ToolFailureCategory::External,
        "weather_api_error" | "web_search_api_error" => ToolFailureCategory::External,
        "rate_limited" => ToolFailureCategory::PolicyDenied,
        "command_not_found" | "permission_denied" | "spawn_failed" | "cargo_spawn_failed" => {
            ToolFailureCategory::External
        }
        "read_file_invalid_range" | "read_file_count_total_too_large" => {
            ToolFailureCategory::InvalidInput
        }
        "read_file_not_file"
        | "read_file_io"
        | "read_file_utf8_decode"
        | "read_file_encoding"
        | "read_file_internal" => ToolFailureCategory::External,
        c if c.starts_with("read_file_workspace_") => ToolFailureCategory::Workspace,
        // file_core 只读族（read_dir / glob_files / list_tree / read_binary_meta）：
        // 「不是目录 / 不是文件」的前置条件归 `invalid_input`；路径越界（或工作区自身不可解析）归 `workspace`；
        // 目标不存在、打开失败等 IO 侧原因归 `external`；其余入参问题统一 `invalid_args`（见上方通用分支）。
        "read_dir_path_not_dir"
        | "glob_files_path_not_dir"
        | "list_tree_path_not_dir"
        | "read_binary_meta_path_not_file" => ToolFailureCategory::InvalidInput,
        "read_dir_sensitive_path" => ToolFailureCategory::PolicyDenied,
        // 临时脚本落到工作区根之外：安全拒绝（不是 IO 故障，也不是参数问题）。
        "python_snippet_temp_path_outside_workspace" => ToolFailureCategory::PolicyDenied,
        // 仅表示「路径落到工作区根之外」或工作区自身不可解析；「文件不存在」走下面的 `*_io_failed`。
        "read_dir_workspace_resolve_failed"
        | "glob_files_workspace_resolve_failed"
        | "glob_files_workspace_root_resolve_failed"
        | "list_tree_workspace_resolve_failed"
        | "list_tree_workspace_root_resolve_failed"
        | "read_binary_meta_workspace_resolve_failed" => ToolFailureCategory::Workspace,
        // 显式登记（不依赖 `ends_with("_failed")` 兜底），改码名时不至于静默退化成 `Unknown`。
        "read_dir_io_failed"
        | "glob_files_io_failed"
        | "list_tree_io_failed"
        | "read_binary_meta_io_failed" => ToolFailureCategory::External,
        "write_precheck_failed" => ToolFailureCategory::InvalidInput,
        "search_in_files_invalid_regex"
        | "search_in_files_invalid_glob"
        | "regex_test_invalid_regex" => ToolFailureCategory::InvalidInput,
        "search_in_files_workspace_base_resolve_failed"
        | "search_in_files_workspace_subpath_resolve_failed"
        | "search_in_files_workspace_outside_root"
        | "search_in_files_path_absolute_not_allowed"
        | "markdown_check_links_workspace_failed"
        | "structured_diff_path_a_resolve_failed"
        | "structured_diff_path_b_resolve_failed"
        // `ToolContext` 未注入该工具所需运行时（如 `cfg` / semantic host / 长期记忆 host）；
        // 属执行环境缺失而非参数错误，故不归 `InvalidInput`。
        | "tool_context_incomplete" => ToolFailureCategory::Workspace,
        // 显式登记：不依赖 `ends_with("_failed")` 兜底，改码名时不至于静默退化成 `Unknown`。
        "text_diff_io_failed" | "table_text_io_failed" | "table_text_parse_failed"
        | "markdown_check_links_failed" | "structured_io_failed" | "structured_parse_failed"
        | "structured_patch_write_failed" | "structured_patch_parse_failed"
        | "structured_patch_serialize_failed" => {
            ToolFailureCategory::External
        }
        // `archive_*` 仅在 `archive-tools` feature 下注册；`not_found` 表示「目标不存在」，
        // 与 `file_core` 里「不存在 → IO 侧」的口径一致，故同归 `External`。
        "archive_not_found" | "archive_pack_failed" | "archive_unpack_failed"
        | "archive_list_failed" => ToolFailureCategory::External,
        // `rust_analyzer`（LSP）：目标文件不存在 / 读源码失败 / 子进程起不来 / LSP 协议层或
        // 响应自带 `error` 都归 `External`；工作区根自身不可解析归 `Workspace`。
        // `rust_analyzer_file_not_found` 必须显式登记——它不以 `_failed` 结尾，走兜底会变 `Unknown`。
        "rust_analyzer_file_not_found"
        | "rust_analyzer_read_failed"
        | "rust_analyzer_spawn_failed"
        | "rust_analyzer_lsp_failed" => ToolFailureCategory::External,
        "rust_analyzer_workspace_resolve_failed" => ToolFailureCategory::Workspace,
        // `reminder` / `event`：`id` 不存在只可能来自调用方给错 id（日程文件里的 id 由工具自己生成），
        // 模型可先 `action=list` 自查后重试，故归 `InvalidInput` 而非 `External`。
        "reminder_not_found" | "event_not_found" => ToolFailureCategory::InvalidInput,
        // 日程 / 提醒的本地 JSON 持久化（两族共用 `schedule.rs` 的 IO helper）：
        // 建目录 / 读 / 写失败归 `External`（文件系统状态），文件内容解析失败另有 `_parse_failed`。
        "schedule_io_failed" | "schedule_parse_failed" => ToolFailureCategory::External,
        // `*_serialize_failed` / `*_internal` 由 [`ToolError::internal_code`] 构造（`category = Internal`），
        // 必须排在 `ends_with("_failed")` 之前，否则重算分类会得到 `External`，与 `ToolError.category` 冲突。
        c if c.ends_with("_serialize_failed") || c.ends_with("_internal") => {
            ToolFailureCategory::Internal
        }
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
        let category = failure_category_for_error_code(code.as_str());
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

    /// 工作区路径解析 / 越界失败（`error_code`：`workspace_resolve_failed`；正文保持 `错误：…`）。
    pub fn path_resolve_failed(e: crate::cm_tools::workspace::path::WorkspacePathError) -> Self {
        Self::external_code("workspace_resolve_failed", e.prefixed_user_message())
    }

    /// 写盘前校验失败：正文沿用 Legacy 文案，并按正文推断 `write_precheck_failed` 等错误码。
    pub fn write_precheck_failed(tool: &str, message: String) -> Self {
        let parsed = super::parse_legacy_output(tool, &message);
        Self::from_parsed_legacy(tool, &parsed, message)
    }

    /// 任意错误码的失败（`category` 由 [`failure_category_for_error_code`] 推导）。
    ///
    /// 供没有专属构造器的码使用（如 `reminder_not_found` 归 `InvalidInput`、`schedule_io_failed`
    /// 归 `External`）。`code` 必须在分类表里有显式分支，否则 `category` 会退化为 `Unknown`。
    pub fn with_code(code: &'static str, message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.to_string()),
        };
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        Self {
            category: failure_category_for_error_code(code),
            code: code.to_string(),
            message,
            retryable,
            legacy_parsed: parsed,
        }
    }

    /// 工作区布局或前置条件问题（如缺少 `Cargo.toml`）；`code` 须与 [`failure_category_for_error_code`] 一致。
    pub fn workspace(code: &'static str, message: String) -> Self {
        Self::with_code(code, message)
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

    /// 需要用户审批（`error_code`：`approval_required`）。
    pub fn approval_required(message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some("approval_required".to_string()),
        };
        Self {
            category: ToolFailureCategory::PolicyDenied,
            code: "approval_required".to_string(),
            message,
            retryable: false,
            legacy_parsed: parsed,
        }
    }

    /// 子进程非零退出：`error_code` 为 `{tool_code}_failed`（如 `cargo_check_failed` /
    /// `pytest_failed` / `python_snippet_failed`），并把退出码写进 `legacy_parsed.exit_code`。
    pub fn subprocess_failed(tool_code: &str, exit_code: i32, message: String) -> Self {
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

    /// 外部工具/IO 类失败（`error_code` 由调用方指定，须与 [`failure_category_for_error_code`] 一致）。
    pub fn external_code(code: &'static str, message: String) -> Self {
        Self::with_code(code, message)
    }

    /// 同 [`Self::external_code`]，但 `code` 由运行时拼接（如 `gh_pr_failed`）。
    ///
    /// 分类仍按 [`failure_category_for_error_code`] 推导，因此 `code` 必须以 `_failed` 结尾
    /// 或已在分类表显式登记——否则会退化成 `Unknown`。调用点目前是 `gh_*` 与 `git` 族
    /// （子命令名 / `action` 名进码名，`&'static str` 表达不了）。
    pub fn external_code_owned(code: String, message: String) -> Self {
        let parsed = ParsedLegacyOutput {
            ok: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error_code: Some(code.clone()),
        };
        let retryable = tool_error_retryable_heuristic(parsed.error_code.as_deref());
        Self {
            category: failure_category_for_error_code(code.as_str()),
            code,
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

    /// 子进程会话超时 / 取消（供 `run_command` 与 `run_and_format*` 类工具复用）：
    /// `code` 为 `timeout` / `cancelled`，`message` 已含截断后的部分输出；
    /// `legacy_parsed` 保留 stdout/stderr（供信封失败路径免再解析）。
    pub fn session_stop(code: &'static str, message: String) -> Self {
        let mut parsed = super::parse_legacy_output(code, &message);
        parsed.ok = false;
        parsed.error_code = Some(code.to_string());
        Self {
            category: ToolFailureCategory::Timeout,
            code: code.to_string(),
            message,
            retryable: code == "timeout",
            legacy_parsed: parsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_tools::tool_result::parse_legacy_output;

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

    #[test]
    fn failure_category_maps_workflow_and_approval_codes() {
        assert_eq!(
            failure_category_for_error_code("workflow_tool_join_error"),
            ToolFailureCategory::External
        );
        assert_eq!(
            failure_category_for_error_code("approval_required"),
            ToolFailureCategory::PolicyDenied
        );
    }

    #[test]
    fn failure_category_maps_network_error_codes() {
        assert_eq!(
            failure_category_for_error_code("http_timeout"),
            ToolFailureCategory::Timeout
        );
        assert_eq!(
            failure_category_for_error_code("http_network_error"),
            ToolFailureCategory::External
        );
        assert_eq!(
            failure_category_for_error_code("http_body_read_error"),
            ToolFailureCategory::External
        );
        assert_eq!(
            failure_category_for_error_code("weather_api_error"),
            ToolFailureCategory::External
        );
        assert_eq!(
            failure_category_for_error_code("web_search_api_error"),
            ToolFailureCategory::External
        );
    }
}
