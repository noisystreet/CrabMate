//! 内建工具名的**单一真源**，位于不依赖任何 crate 内模块的最底层。
//!
//! 工具名是下列各面的共同主键，此前它们各自写字面量：
//!
//! - `cm_tools::tools::tool_specs_registry::specs/*.inc.rs` 的 `ToolSpec.name`（声明点）
//! - [`crate::cm_tools::registry_policy`] 的只读批 / 重试 / 门控判定
//! - [`crate::cm_tools::tools::dev_tag`] 的 `Development` 子域标签
//! - [`crate::cm_tools::tool_dispatch`] 的工具名 → `HandlerId` 分派映射表
//! - [`crate::cm_config::tool_write_effect`] 的写副作用归类
//! - permission 策略与 turn allowlist、审批键、`tool_summary`、SSE 事件与 MCP 暴露面
//!
//! 任一处拼写与声明点不一致都会**静默失效**（不报错、不命中），且人工无法核对
//! 67 个名字 × 多处声明。因此：**新增工具必须先在此声明常量，再让各处引用**，
//! 而不是在调用点写字面量。
//!
//! 一致性由 `tool_specs_registry` 的守卫测试 `all_tool_names_match_specs` 保证：
//! 本模块的 [`ALL`] 与 `tool_specs()` 必须双向一致（无遗漏、无多余）。
//!
//! ## 为什么是独立顶层模块，而不是放在 `cm_tools` 或 `cm_config` 里
//!
//! 消费者横跨两层：`cm_config::tool_write_effect` 与 `cm_tools` / `cm_agent` /
//! `cm_internal` 各处引用点。放在 `cm_tools` 里会让 `cm_config` 反向依赖上层
//! （`cm_tools` 已依赖 `cm_config`，成环）；放在 `cm_config` 里虽不成环，但工具名是
//! **标识符**而非配置项，且 `cm_config` 属 `#[doc(hidden)]` 的 server 实现层 —— 将来
//! `protocol` 侧若需要同一份名字就得再搬一次。
//!
//! 因此本模块保持**零依赖**（只有 `&'static str` 与 `#[cfg(feature = ...)]`），
//! 只允许被上层单向引用；`scripts/check-crate-deps.sh` 强制这条边。
//!
//! 前缀 / 来源判定（`mcp__` / `dyn__` 与 [`crate::cm_tools::tool_naming::ToolOrigin`]）
//! **不在**本模块，见 [`crate::cm_tools::tool_naming`]——那里判的是「名字的形状」，
//! 这里列的是「内建工具的全部名字」。

// --- `specs/archive.inc.rs`（feature `archive-tools`）---
/// `archive_pack`
pub const ARCHIVE_PACK: &str = "archive_pack";
/// `archive_unpack`
pub const ARCHIVE_UNPACK: &str = "archive_unpack";
/// `archive_list`
pub const ARCHIVE_LIST: &str = "archive_list";

// --- `specs/basic_network.inc.rs` ---
/// `get_weather`
pub const GET_WEATHER: &str = "get_weather";
/// `web_search`
pub const WEB_SEARCH: &str = "web_search";
/// `http_fetch`
pub const HTTP_FETCH: &str = "http_fetch";
/// `http_request`
pub const HTTP_REQUEST: &str = "http_request";

// --- `specs/cargo_rust.inc.rs` ---
/// `cargo_check`
pub const CARGO_CHECK: &str = "cargo_check";
/// `cargo_test`
pub const CARGO_TEST: &str = "cargo_test";
/// `cargo_clippy`
pub const CARGO_CLIPPY: &str = "cargo_clippy";
/// `cargo_run`
pub const CARGO_RUN: &str = "cargo_run";
/// `rust_analyzer`
pub const RUST_ANALYZER: &str = "rust_analyzer";

// --- `specs/diagnostics_docs.inc.rs` ---
/// `present_clarification_questionnaire`
pub const PRESENT_CLARIFICATION_QUESTIONNAIRE: &str = "present_clarification_questionnaire";
/// `diagnostic_summary`
pub const DIAGNOSTIC_SUMMARY: &str = "diagnostic_summary";
/// `repo_overview_sweep`
pub const REPO_OVERVIEW_SWEEP: &str = "repo_overview_sweep";
/// `crate_contract_map`
pub const CRATE_CONTRACT_MAP: &str = "crate_contract_map";
/// `long_term_remember`
pub const LONG_TERM_REMEMBER: &str = "long_term_remember";
/// `long_term_forget`
pub const LONG_TERM_FORGET: &str = "long_term_forget";
/// `long_term_memory_list`
pub const LONG_TERM_MEMORY_LIST: &str = "long_term_memory_list";
/// `summarize_experience`
pub const SUMMARIZE_EXPERIENCE: &str = "summarize_experience";

// --- `specs/docs_health.inc.rs` ---
/// `docs_health_sweep`
pub const DOCS_HEALTH_SWEEP: &str = "docs_health_sweep";

// --- `specs/exec_package.inc.rs` ---
/// `run_command`
pub const RUN_COMMAND: &str = "run_command";
/// `terminal_session`
pub const TERMINAL_SESSION: &str = "terminal_session";

// --- `specs/file_core.inc.rs` ---
/// `create_file`
pub const CREATE_FILE: &str = "create_file";
/// `modify_file`
pub const MODIFY_FILE: &str = "modify_file";
/// `copy_file`
pub const COPY_FILE: &str = "copy_file";
/// `move_file`
pub const MOVE_FILE: &str = "move_file";
/// `read_file`
pub const READ_FILE: &str = "read_file";
/// `read_dir`
pub const READ_DIR: &str = "read_dir";
/// `glob_files`
pub const GLOB_FILES: &str = "glob_files";
/// `list_tree`
pub const LIST_TREE: &str = "list_tree";
/// `read_binary_meta`
pub const READ_BINARY_META: &str = "read_binary_meta";
/// `apply_patch`
pub const APPLY_PATCH: &str = "apply_patch";
/// `search_in_files`
pub const SEARCH_IN_FILES: &str = "search_in_files";
/// `codebase_semantic_search`
pub const CODEBASE_SEMANTIC_SEARCH: &str = "codebase_semantic_search";

// --- `specs/file_extra.inc.rs` ---
/// `delete_files`
pub const DELETE_FILES: &str = "delete_files";
/// `delete_dir`
pub const DELETE_DIR: &str = "delete_dir";
/// `append_file`
pub const APPEND_FILE: &str = "append_file";
/// `create_dir`
pub const CREATE_DIR: &str = "create_dir";
/// `search_replace`
pub const SEARCH_REPLACE: &str = "search_replace";

// --- `specs/frontend_audit_ci.inc.rs` ---
/// `workflow_execute`
pub const WORKFLOW_EXECUTE: &str = "workflow_execute";

// --- `specs/frontend_python.inc.rs` ---
/// `pytest_run`
pub const PYTEST_RUN: &str = "pytest_run";
/// `python_snippet_run`
pub const PYTHON_SNIPPET_RUN: &str = "python_snippet_run";

// --- `specs/github_cli.inc.rs` ---
/// `gh_pr`
pub const GH_PR: &str = "gh_pr";
/// `gh_run`
pub const GH_RUN: &str = "gh_run";
/// `gh_issue_create`
pub const GH_ISSUE_CREATE: &str = "gh_issue_create";
/// `gh_release_create`
pub const GH_RELEASE_CREATE: &str = "gh_release_create";
/// `gh_api`
pub const GH_API: &str = "gh_api";

// --- `specs/git_read.inc.rs` ---
/// `git`
pub const GIT: &str = "git";

// --- `specs/markdown_structured.inc.rs` ---
/// `markdown_check_links`
pub const MARKDOWN_CHECK_LINKS: &str = "markdown_check_links";
/// `structured`
pub const STRUCTURED: &str = "structured";

// --- `specs/metrics.inc.rs` ---
/// `code_stats`
pub const CODE_STATS: &str = "code_stats";
/// `dependency_graph`
pub const DEPENDENCY_GRAPH: &str = "dependency_graph";
/// `coverage_report`
pub const COVERAGE_REPORT: &str = "coverage_report";

// --- `specs/misc_basic.inc.rs` ---
/// `regex_test`
pub const REGEX_TEST: &str = "regex_test";
/// `date_calc`
pub const DATE_CALC: &str = "date_calc";
/// `json_format`
pub const JSON_FORMAT: &str = "json_format";
/// `env_var_check`
pub const ENV_VAR_CHECK: &str = "env_var_check";

// --- `specs/process.inc.rs` ---
/// `background_job`
pub const BACKGROUND_JOB: &str = "background_job";
/// `background_job_cancel`
pub const BACKGROUND_JOB_CANCEL: &str = "background_job_cancel";

// --- `specs/schedule.inc.rs` ---
/// `reminder`
pub const REMINDER: &str = "reminder";
/// `event`
pub const EVENT: &str = "event";

// --- `specs/self_config_info.inc.rs` ---
/// `self_config_info`
pub const SELF_CONFIG_INFO: &str = "self_config_info";
/// `skill_manage`
pub const SKILL_MANAGE: &str = "skill_manage";

// --- `specs/text_code_nav.inc.rs` ---
/// `text_diff`
pub const TEXT_DIFF: &str = "text_diff";
/// `table_text`
pub const TABLE_TEXT: &str = "table_text";
/// `code_nav`
pub const CODE_NAV: &str = "code_nav";

/// 全部内建工具名。
///
/// 用于遍历与守卫测试；**顺序不承载语义**（注册顺序由 `tool_specs_registry` 决定）。
/// `archive-tools` 关闭时归档三项不在注册表中，故此处同样按 feature 条件包含。
pub static ALL: &[&str] = &[
    #[cfg(feature = "archive-tools")]
    ARCHIVE_PACK,
    #[cfg(feature = "archive-tools")]
    ARCHIVE_UNPACK,
    #[cfg(feature = "archive-tools")]
    ARCHIVE_LIST,
    GET_WEATHER,
    WEB_SEARCH,
    HTTP_FETCH,
    HTTP_REQUEST,
    CARGO_CHECK,
    CARGO_TEST,
    CARGO_CLIPPY,
    CARGO_RUN,
    RUST_ANALYZER,
    PRESENT_CLARIFICATION_QUESTIONNAIRE,
    DIAGNOSTIC_SUMMARY,
    REPO_OVERVIEW_SWEEP,
    CRATE_CONTRACT_MAP,
    LONG_TERM_REMEMBER,
    LONG_TERM_FORGET,
    LONG_TERM_MEMORY_LIST,
    SUMMARIZE_EXPERIENCE,
    DOCS_HEALTH_SWEEP,
    RUN_COMMAND,
    TERMINAL_SESSION,
    CREATE_FILE,
    MODIFY_FILE,
    COPY_FILE,
    MOVE_FILE,
    READ_FILE,
    READ_DIR,
    GLOB_FILES,
    LIST_TREE,
    READ_BINARY_META,
    APPLY_PATCH,
    SEARCH_IN_FILES,
    CODEBASE_SEMANTIC_SEARCH,
    DELETE_FILES,
    DELETE_DIR,
    APPEND_FILE,
    CREATE_DIR,
    SEARCH_REPLACE,
    WORKFLOW_EXECUTE,
    PYTEST_RUN,
    PYTHON_SNIPPET_RUN,
    GH_PR,
    GH_RUN,
    GH_ISSUE_CREATE,
    GH_RELEASE_CREATE,
    GH_API,
    GIT,
    MARKDOWN_CHECK_LINKS,
    STRUCTURED,
    CODE_STATS,
    DEPENDENCY_GRAPH,
    COVERAGE_REPORT,
    REGEX_TEST,
    DATE_CALC,
    JSON_FORMAT,
    ENV_VAR_CHECK,
    BACKGROUND_JOB,
    BACKGROUND_JOB_CANCEL,
    REMINDER,
    EVENT,
    SELF_CONFIG_INFO,
    SKILL_MANAGE,
    TEXT_DIFF,
    TABLE_TEXT,
    CODE_NAV,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_duplicate_names() {
        let mut seen = HashSet::new();
        for name in ALL {
            assert!(seen.insert(*name), "工具名 `{name}` 在 ALL 中重复");
        }
    }

    #[test]
    fn every_name_is_non_empty_snake_case() {
        for name in ALL {
            assert!(!name.is_empty(), "存在空工具名");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "工具名 `{name}` 不符合 snake_case 约定"
            );
        }
    }
}
