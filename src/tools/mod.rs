//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod cargo_tools;
mod ci_tools;
mod code_metrics;
mod code_nav;
mod command;
mod container_tools;
mod date_calc;
mod debug_tools;
mod diagnostics;
mod docs_health_sweep;
pub(crate) use diagnostics::capture_trimmed;
pub(crate) use parse_args::parse_args_json;
mod env_var_check;
mod error_playbook;
mod exec;
mod file;
pub(crate) use file::canonical_workspace_root;
mod format;
mod frontend_tools;
mod git;
mod github_cli;
mod go_tools;
mod grep;
mod grep_try;
pub mod http_fetch;
mod json_format;
mod jvm_tools;
mod lint;
mod markdown_links;
mod nodejs_tools;
pub(crate) mod output_util;
mod package_query;
mod parse_args;
mod patch;
mod precommit_tools;
mod process_tools;
mod python_tools;
mod quality_tools;
mod regex_test;
mod release_docs;
mod repo_overview;
mod rust_ide;
mod schedule;
mod schema_check;
mod security_tools;
mod source_analysis_tools;
mod spell_astgrep_tools;
mod structured_data;
mod symbol;
mod table_text;
mod test_result_cache;
mod text_diff;
mod text_transform;
mod time;
mod todo_scan;
mod tool_params;
mod tool_specs_registry;
mod tool_summary;
mod tool_summary_args;
mod unit_convert;
mod weather;
mod web_search;

pub mod dev_tag;

use std::path::Path;
use std::sync::Arc;

use crate::config::{AgentConfig, ExposeSecret};
use crate::path_workspace::{validate_effective_workspace_base, validate_workspace_set_path};
use crate::tool_result::{ToolError, ToolResult};
use crate::types::{FunctionDef, Tool};
use crate::workspace_changelist::WorkspaceChangelist;

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：时间/计算/天气、联网搜索与受控 HTTP、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

pub struct ToolContext<'a> {
    /// 代码语义检索参数；主 Agent 路径由 `tool_context_for*` 从 [`AgentConfig`] 填充，其它路径为 `None` 时该工具不可用。
    pub codebase_semantic: Option<crate::codebase_semantic_index::CodebaseSemanticToolParams>,
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    pub allowed_commands: &'a [String],
    pub working_dir: &'a std::path::Path,
    pub web_search_timeout_secs: u64,
    pub web_search_provider: crate::config::WebSearchProvider,
    pub web_search_api_key: &'a str,
    pub web_search_max_results: u32,
    pub http_fetch_allowed_prefixes: &'a [String],
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    /// 单轮 `run_agent_turn` 内 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<&'a crate::read_file_turn_cache::ReadFileTurnCache>,
    /// 本会话工作区变更集（按 `long_term_memory_scope_id`）；`None` 时不记录。
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// `cargo_test` / `npm run test` / 部分 `run_command cargo test` 的进程内输出缓存。
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
/// 与内置文件工具相同的路径规则：将相对路径解析为工作区内绝对路径（供变更集等跨模块只读）。
pub use crate::path_workspace::WorkspacePathError;

pub fn resolve_workspace_path_for_read(
    working_dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, WorkspacePathError> {
    file::resolve_for_read(working_dir, rel)
}

/// REPL **`/workspace`** / **`/cd`**：相对路径走 [`resolve_workspace_path_for_read`]（与 `read_file` 等一致：**禁止**以 `/` 开头的绝对路径）；绝对路径走 [`crate::path_workspace::validate_workspace_set_path`]（与 Web **`POST /workspace`** 一致：`workspace_allowed_roots` + 敏感目录黑名单）。
pub fn resolve_repl_workspace_switch_path(
    cfg: &AgentConfig,
    current_work_dir: &Path,
    raw: &str,
) -> Result<std::path::PathBuf, ReplWorkspaceSwitchError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ReplWorkspaceSwitchError::Usage);
    }
    if Path::new(raw).is_absolute() {
        validate_workspace_set_path(cfg, raw).map_err(ReplWorkspaceSwitchError::Path)
    } else {
        let p = resolve_workspace_path_for_read(current_work_dir, raw)
            .map_err(ReplWorkspaceSwitchError::Path)?;
        if !p.is_dir() {
            return Err(ReplWorkspaceSwitchError::NotADirectory(
                p.display().to_string(),
            ));
        }
        validate_effective_workspace_base(cfg, &p).map_err(ReplWorkspaceSwitchError::Path)?;
        Ok(p)
    }
}

/// REPL `/workspace` 切换失败：用法提示或与 [`WorkspacePathError`] 同源的路径策略错误。
#[derive(Debug, thiserror::Error)]
pub enum ReplWorkspaceSwitchError {
    #[error("用法: /workspace <路径>（须为已存在目录）")]
    Usage,
    #[error("不是目录: {0}")]
    NotADirectory(String),
    #[error(transparent)]
    Path(#[from] WorkspacePathError),
}

pub fn tool_context_for<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
) -> ToolContext<'a> {
    ToolContext {
        codebase_semantic: Some(
            crate::codebase_semantic_index::CodebaseSemanticToolParams::from_agent_config(cfg),
        ),
        command_max_output_len: cfg.command_max_output_len,
        weather_timeout_secs: cfg.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search_timeout_secs,
        web_search_provider: cfg.web_search_provider,
        web_search_api_key: cfg.web_search_api_key.expose_secret(),
        web_search_max_results: cfg.web_search_max_results,
        http_fetch_allowed_prefixes: cfg.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch_max_response_bytes,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: cfg.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.test_result_cache_max_entries,
    }
}

/// 与 [`tool_context_for`] 相同，但可挂载单轮 `read_file` 缓存与会话变更集（供 `dispatch_tool` / `execute_tools`）。
pub fn tool_context_for_with_read_cache<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<&'a crate::read_file_turn_cache::ReadFileTurnCache>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
) -> ToolContext<'a> {
    ToolContext {
        read_file_turn_cache,
        workspace_changelist,
        ..tool_context_for(cfg, allowed_commands, working_dir)
    }
}

type ToolRunner = fn(args_json: &str, ctx: &ToolContext<'_>) -> String;
type ParamBuilder = fn() -> serde_json::Value;

/// 工具调用摘要类型：用于前端 Chat 面板展示。
#[derive(Clone, Copy)]
pub(super) enum ToolSummaryKind {
    /// 无自定义摘要。
    None,
    /// 固定摘要字符串（与参数无关）。
    Static(&'static str),
    /// 从解析后的 args JSON 动态生成摘要。
    Dynamic(fn(&serde_json::Value) -> Option<String>),
}

#[derive(Clone, Copy)]
struct ToolSpec {
    name: &'static str,
    description: &'static str,
    category: ToolCategory,
    parameters: ParamBuilder,
    runner: ToolRunner,
    summary: ToolSummaryKind,
}

fn runner_get_current_time(args: &str, _ctx: &ToolContext<'_>) -> String {
    let v: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Object(Default::default()),
    };
    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .and_then(time::TimeOutputMode::from_str)
        .unwrap_or(time::TimeOutputMode::Time);
    let year = v.get("year").and_then(|y| y.as_i64()).map(|y| y as i32);
    let month = v
        .get("month")
        .and_then(|m| m.as_u64())
        .and_then(|m| u32::try_from(m).ok());
    time::run(mode, year, month)
}

fn runner_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    let expr = match serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| {
            v.get("expression")
                .and_then(|e| e.as_str())
                .map(String::from)
        }) {
        Some(s) => s,
        None => return "错误：缺少 expression 参数".to_string(),
    };
    calc::run(&expr)
}

fn runner_convert_units(args: &str, _ctx: &ToolContext<'_>) -> String {
    unit_convert::run(args)
}

fn runner_get_weather(args: &str, ctx: &ToolContext<'_>) -> String {
    weather::run(args, ctx.weather_timeout_secs)
}

fn runner_web_search(args: &str, ctx: &ToolContext<'_>) -> String {
    web_search::run(args, ctx)
}

fn runner_http_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_direct(args, ctx)
}

fn runner_http_request(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_request_direct(args, ctx)
}

fn runner_run_command(args: &str, ctx: &ToolContext<'_>) -> String {
    let test_cache = ctx
        .test_result_cache_enabled
        .then_some(command::RunCommandTestCacheOpts {
            enabled: true,
            max_entries: ctx.test_result_cache_max_entries,
            workspace_root: ctx.working_dir,
        });
    command::run(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
        test_cache,
    )
}

fn runner_run_executable(args: &str, ctx: &ToolContext<'_>) -> String {
    exec::run(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_package_query(args: &str, ctx: &ToolContext<'_>) -> String {
    package_query::run(args, ctx.command_max_output_len)
}

fn runner_gh_pr_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_pr_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_issue_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_issue_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_issue_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_issue_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_run_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_run_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_pr_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_diff(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_run_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_run_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_release_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_release_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_release_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_release_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_search(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_search(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_gh_api(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_api(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_cargo_check(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_test(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_test(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

fn runner_cargo_clippy(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clippy(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_metadata(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_metadata(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_tree(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_clean(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clean(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_doc(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_doc(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_nextest(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_nextest(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_fmt_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::cargo_fmt_check_tool(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_outdated(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_outdated(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_machete(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_machete(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_udeps(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_udeps(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_publish_dry_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_publish_dry_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_compiler_json(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_compiler_json(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_analyzer_goto_definition(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_goto_definition(args, ctx.working_dir)
}

fn runner_rust_analyzer_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_find_references(args, ctx.working_dir)
}

fn runner_rust_analyzer_hover(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_hover(args, ctx.working_dir)
}

fn runner_rust_analyzer_document_symbol(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_document_symbol(args, ctx.working_dir)
}

fn runner_cargo_fix(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_fix(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_rust_test_one(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::rust_test_one(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

fn runner_ruff_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::ruff_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_pytest_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::pytest_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_mypy_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::mypy_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_python_install_editable(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::python_install_editable(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_uv_sync(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_sync(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_uv_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_go_build(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_build(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_go_test(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_go_vet(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_vet(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_go_mod_tidy(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_mod_tidy(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_go_fmt_check(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_fmt_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_maven_compile(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::maven_compile(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_maven_test(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::maven_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_gradle_compile(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::gradle_compile(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_gradle_test(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::gradle_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_docker_build(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::docker_build(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_docker_compose_ps(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::docker_compose_ps(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_podman_images(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::podman_images(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_pre_commit_run(args: &str, ctx: &ToolContext<'_>) -> String {
    precommit_tools::pre_commit_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_typos_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::typos_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_codespell_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::codespell_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ast_grep_run(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ast_grep_rewrite(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_rewrite(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_lint(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_lint(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_build(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_build(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_frontend_test(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_test(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_audit(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_audit(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cargo_deny(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_deny(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_backtrace_analyze(args: &str, _ctx: &ToolContext<'_>) -> String {
    debug_tools::rust_backtrace_analyze(args)
}

fn runner_diagnostic_summary(args: &str, ctx: &ToolContext<'_>) -> String {
    diagnostics::diagnostic_summary(args, ctx.working_dir)
}

fn runner_error_output_playbook(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::error_output_playbook(args, ctx.allowed_commands)
}

fn runner_playbook_run_commands(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::playbook_run_commands(args, ctx)
}

fn runner_changelog_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::changelog_draft(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_license_notice(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::license_notice(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_repo_overview_sweep(args: &str, ctx: &ToolContext<'_>) -> String {
    repo_overview::repo_overview_sweep(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_docs_health_sweep(args: &str, ctx: &ToolContext<'_>) -> String {
    docs_health_sweep::docs_health_sweep(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_ci_pipeline_local(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::ci_pipeline_local(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_release_ready_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::release_ready_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_workflow_execute(_args: &str, _ctx: &ToolContext<'_>) -> String {
    // 由 runtime 在 run_agent_turn 中拦截实际执行。
    "workflow_execute：由运行时引擎执行（若你看到这条，说明拦截未生效）。".to_string()
}

/// 生成 `fn runner_git_* -> git::impl(args, max_len, cwd)`；新增 Git 工具时在列表中增一行并注册 `tool_specs_registry`。
macro_rules! define_git_runner {
    ($runner:ident, $git_fn:ident) => {
        fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
            git::$git_fn(args, ctx.command_max_output_len, ctx.working_dir)
        }
    };
}

macro_rules! define_git_runners {
    ($( $runner:ident => $git_fn:ident ),* $(,)? ) => {
        $( define_git_runner!($runner, $git_fn); )*
    };
}

define_git_runners! {
    runner_git_status => status,
    runner_git_diff => diff,
    runner_git_clean_check => clean_check,
    runner_git_diff_stat => diff_stat,
    runner_git_diff_names => diff_names,
    runner_git_log => log,
    runner_git_show => show,
    runner_git_diff_base => diff_base,
    runner_git_blame => blame,
    runner_git_file_history => file_history,
    runner_git_branch_list => branch_list,
    runner_git_remote_status => remote_status,
    runner_git_stage_files => stage_files,
    runner_git_commit => commit,
    runner_git_fetch => fetch,
    runner_git_remote_list => remote_list,
    runner_git_remote_set_url => remote_set_url,
    runner_git_apply => apply,
    runner_git_clone => clone_repo,
    runner_git_checkout => checkout,
    runner_git_branch_create => branch_create,
    runner_git_branch_delete => branch_delete,
    runner_git_push => push,
    runner_git_merge => merge,
    runner_git_rebase => rebase,
    runner_git_stash => stash,
    runner_git_tag => tag,
    runner_git_reset => reset,
    runner_git_cherry_pick => cherry_pick,
    runner_git_revert => revert,
}

fn runner_create_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_file(args, ctx.working_dir, ctx)
}

fn runner_modify_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::modify_file(args, ctx.working_dir, ctx)
}

fn runner_copy_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::copy_file(args, ctx.working_dir, ctx)
}

fn runner_move_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::move_file(args, ctx.working_dir, ctx)
}

fn runner_read_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_file(args, ctx.working_dir, ctx)
}

#[allow(clippy::result_large_err)]
fn read_file_try_dispatch(
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::tool_result::ToolError> {
    file::read_file_try(args_json, ctx.working_dir, ctx)
}

fn runner_read_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_dir(args, ctx.working_dir)
}

fn runner_glob_files(args: &str, ctx: &ToolContext<'_>) -> String {
    file::glob_files(args, ctx.working_dir)
}

fn runner_list_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    file::list_tree(args, ctx.working_dir)
}

fn runner_file_exists(args: &str, ctx: &ToolContext<'_>) -> String {
    file::file_exists(args, ctx.working_dir)
}

fn runner_read_binary_meta(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_binary_meta(args, ctx.working_dir)
}

fn runner_hash_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::hash_file(args, ctx.working_dir)
}

fn runner_extract_in_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::extract_in_file(args, ctx.working_dir)
}

fn runner_apply_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    patch::run_with_changelist(args, ctx.working_dir, ctx.workspace_changelist)
}

fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
}

fn runner_codebase_semantic_search(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(p) = ctx.codebase_semantic.as_ref() else {
        return "错误：当前执行环境未注入代码语义检索配置，无法使用 codebase_semantic_search（如部分工作流节点路径）"
            .to_string();
    };
    crate::codebase_semantic_index::run_tool(args, ctx.working_dir, p, ctx.command_max_output_len)
}

fn runner_markdown_check_links(args: &str, ctx: &ToolContext<'_>) -> String {
    markdown_links::markdown_check_links(args, ctx.working_dir)
}

fn runner_structured_validate(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_validate(args, ctx.working_dir)
}

fn runner_structured_query(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_query(args, ctx.working_dir)
}

fn runner_structured_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_diff(args, ctx.working_dir)
}

fn runner_structured_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_patch(args, ctx.working_dir, ctx)
}

fn runner_text_transform(args: &str, _ctx: &ToolContext<'_>) -> String {
    text_transform::run(args)
}

fn runner_text_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    text_diff::run(args, ctx.working_dir)
}

fn runner_table_text(args: &str, ctx: &ToolContext<'_>) -> String {
    table_text::run(args, ctx.working_dir)
}

fn runner_find_symbol(args: &str, ctx: &ToolContext<'_>) -> String {
    symbol::run(args, ctx.working_dir)
}

fn runner_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::find_references(args, ctx.working_dir)
}

fn runner_rust_file_outline(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::rust_file_outline(args, ctx.working_dir)
}

fn runner_format_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run(args, ctx.working_dir)
}

fn runner_format_check_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run_check(args, ctx.working_dir)
}

fn runner_run_lints(args: &str, ctx: &ToolContext<'_>) -> String {
    lint::run(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_quality_workspace(args: &str, ctx: &ToolContext<'_>) -> String {
    quality_tools::quality_workspace(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_add_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_reminder(args, ctx.working_dir)
}

fn runner_list_reminders(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_reminders(args, ctx.working_dir)
}

fn runner_complete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::complete_reminder(args, ctx.working_dir)
}

fn runner_delete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_reminder(args, ctx.working_dir)
}

fn runner_update_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_reminder(args, ctx.working_dir)
}

fn runner_add_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_event(args, ctx.working_dir)
}

fn runner_list_events(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_events(args, ctx.working_dir)
}

fn runner_delete_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_event(args, ctx.working_dir)
}

fn runner_update_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_event(args, ctx.working_dir)
}

// ── Node.js / npm / npx ─────────────────────────────────────

fn runner_npm_install(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_install(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_npm_run(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_run(args, ctx.working_dir, ctx.command_max_output_len, ctx)
}
fn runner_npx_run(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npx_run(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_tsc_check(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::tsc_check(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── Go 补充：golangci-lint ──────────────────────────────────

fn runner_golangci_lint(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::golangci_lint(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── 进程与端口管理 ──────────────────────────────────────────

fn runner_port_check(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::port_check(args, ctx.command_max_output_len)
}
fn runner_process_list(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::process_list(args, ctx.command_max_output_len)
}

// ── 代码度量与分析 ──────────────────────────────────────────

// ── 文件增强 ────────────────────────────────────────────────

fn runner_delete_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::delete_file(args, ctx.working_dir, ctx)
}
fn runner_delete_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::delete_dir(args, ctx.working_dir)
}
fn runner_append_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::append_file(args, ctx.working_dir, ctx)
}
fn runner_create_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_dir(args, ctx.working_dir)
}
fn runner_search_replace(args: &str, ctx: &ToolContext<'_>) -> String {
    file::search_replace(args, ctx.working_dir, ctx)
}
fn runner_chmod_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::chmod_file(args, ctx.working_dir)
}
fn runner_symlink_info(args: &str, ctx: &ToolContext<'_>) -> String {
    file::symlink_info(args, ctx.working_dir)
}

fn runner_code_stats(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::code_stats(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_dependency_graph(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::dependency_graph(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_coverage_report(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::coverage_report(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── 新增纯内存 / 开发辅助工具 ────────────────────────────────

fn runner_regex_test(args: &str, _ctx: &ToolContext<'_>) -> String {
    regex_test::run(args)
}

fn runner_date_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    date_calc::run(args)
}

fn runner_json_format(args: &str, _ctx: &ToolContext<'_>) -> String {
    json_format::run(args)
}

fn runner_env_var_check(args: &str, _ctx: &ToolContext<'_>) -> String {
    env_var_check::run(args)
}

fn runner_todo_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    todo_scan::run(args, ctx.working_dir)
}

// ── 源码分析工具 ──────────────────────────────────────────────

fn runner_shellcheck_check(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::shellcheck_check(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_cppcheck_analyze(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::cppcheck_analyze(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_semgrep_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::semgrep_scan(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_hadolint_check(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::hadolint_check(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_bandit_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::bandit_scan(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_lizard_complexity(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::lizard_complexity(args, ctx.working_dir, ctx.command_max_output_len)
}

fn tool_specs() -> &'static [ToolSpec] {
    tool_specs_registry::tool_specs()
}

/// name → tool_specs() 数组下标索引，首次访问时构建，O(1) 查找。
fn tool_spec_index() -> &'static std::collections::HashMap<&'static str, usize> {
    use std::sync::LazyLock;
    static INDEX: LazyLock<std::collections::HashMap<&'static str, usize>> = LazyLock::new(|| {
        tool_specs()
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name, i))
            .collect()
    });
    &INDEX
}

fn find_spec(name: &str) -> Option<&'static ToolSpec> {
    tool_spec_index().get(name).map(|&idx| &tool_specs()[idx])
}

/// 构建工具列表时的分类与开发子域标签过滤。
#[derive(Clone, Copy, Default)]
pub struct ToolsBuildOptions<'a> {
    /// `None` 或 `Some(&[])`：不按顶层分类过滤（`Basic` 与 `Development` 均保留）。
    pub categories: Option<&'a [ToolCategory]>,
    /// `None` 或 `Some(&[])`：不按标签过滤。`Some(non-empty)`：仅保留 **Development** 工具中
    /// [`dev_tag::tags_for_tool_name`] 与列表 **有交集** 者；`Basic` 仍只受 `categories` 约束。
    pub dev_tags: Option<&'a [&'a str]>,
}

fn tool_passes_filters(spec: &ToolSpec, opts: ToolsBuildOptions<'_>) -> bool {
    let cats = opts.categories.unwrap_or(&[]);
    if !cats.is_empty() && !cats.contains(&spec.category) {
        return false;
    }
    let Some(wanted) = opts.dev_tags.and_then(|t| (!t.is_empty()).then_some(t)) else {
        return true;
    };
    if spec.category != ToolCategory::Development {
        return true;
    }
    dev_tag::tags_for_tool_name(spec.name)
        .iter()
        .any(|tag| wanted.contains(tag))
}

/// 构建传给 API 的工具列表（表驱动注册）。
pub fn build_tools() -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions::default())
}

/// 构建传给 API 的工具列表：可按顶层分类过滤（[`ToolCategory::Basic`] / [`ToolCategory::Development`]）。
pub fn build_tools_filtered(allowed: Option<&[ToolCategory]>) -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions {
        categories: allowed,
        dev_tags: None,
    })
}

/// 同时支持顶层分类与 Development 子域标签过滤（见 [`ToolsBuildOptions`]）。
pub fn build_tools_with_options(opts: ToolsBuildOptions<'_>) -> Vec<Tool> {
    tool_specs()
        .iter()
        .filter(|s| tool_passes_filters(s, opts))
        .map(|s| Tool {
            typ: TOOL_TYPE_FUNCTION.to_string(),
            function: FunctionDef {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: cached_params(s),
            },
        })
        .collect()
}

const TOOL_TYPE_FUNCTION: &str = "function";

fn cached_params(spec: &ToolSpec) -> serde_json::Value {
    use std::sync::LazyLock;
    static CACHE: LazyLock<std::collections::HashMap<&'static str, serde_json::Value>> =
        LazyLock::new(|| {
            tool_specs()
                .iter()
                .map(|s| (s.name, (s.parameters)()))
                .collect()
        });
    CACHE
        .get(spec.name)
        .cloned()
        .unwrap_or_else(|| (spec.parameters)())
}

/// 内置工具 `name` 的 parameters JSON Schema（供工作流等静态校验复用）。
pub(crate) fn cached_params_for_tool_name(name: &str) -> Option<serde_json::Value> {
    find_spec(name).map(cached_params)
}

pub(crate) use schema_check::workflow_tool_args_satisfy_required;

/// 执行本地工具并返回结果字符串。
/// `ToolContext` 聚合 `run_command`、`get_weather`、`web_search` 等工具所需的配置项。
pub fn run_tool(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    match find_spec(name) {
        Some(spec) => (spec.runner)(args_json, ctx),
        None => format!("未知工具：{}", name),
    }
}

/// `run_command` 与 `cargo_*` / `rust_test_one` 走显式 [`ToolError`]；其余工具仍经 [`run_tool`] + [`crate::tool_result::parse_legacy_output`]。
#[allow(clippy::result_large_err)]
fn run_tool_dispatch(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::tool_result::ParsedLegacyOutput), ToolError> {
    if find_spec(name).is_none() {
        return Err(ToolError::unknown_tool(name));
    }
    match name {
        "run_command" => {
            let test_cache =
                ctx.test_result_cache_enabled
                    .then_some(command::RunCommandTestCacheOpts {
                        enabled: true,
                        max_entries: ctx.test_result_cache_max_entries,
                        workspace_root: ctx.working_dir,
                    });
            match command::run_try(
                args_json,
                ctx.command_max_output_len,
                ctx.allowed_commands,
                ctx.working_dir,
                test_cache,
            ) {
                Ok(output) => {
                    let parsed = crate::tool_result::parse_legacy_output(name, &output);
                    if parsed.ok {
                        Ok((output, parsed))
                    } else {
                        Err(ToolError::from_parsed_legacy(name, &parsed, output))
                    }
                }
                Err(e) => Err(e),
            }
        }
        "cargo_check" => {
            cargo_tools::cargo_check_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_test" => cargo_tools::cargo_test_try(
            args_json,
            ctx.working_dir,
            ctx.command_max_output_len,
            Some(ctx),
        )
        .map(|output| finish_dispatch_parsed(name, output)),
        "cargo_clippy" => {
            cargo_tools::cargo_clippy_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_metadata" => {
            cargo_tools::cargo_metadata_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_tree" => {
            cargo_tools::cargo_tree_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_clean" => {
            cargo_tools::cargo_clean_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_doc" => {
            cargo_tools::cargo_doc_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_nextest" => {
            cargo_tools::cargo_nextest_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_outdated" => {
            cargo_tools::cargo_outdated_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_machete" => {
            cargo_tools::cargo_machete_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_udeps" => {
            cargo_tools::cargo_udeps_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_publish_dry_run" => cargo_tools::cargo_publish_dry_run_try(
            args_json,
            ctx.working_dir,
            ctx.command_max_output_len,
        )
        .map(|output| finish_dispatch_parsed(name, output)),
        "cargo_fix" => {
            cargo_tools::cargo_fix_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "cargo_run" => {
            cargo_tools::cargo_run_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
        "rust_test_one" => cargo_tools::rust_test_one_try(
            args_json,
            ctx.working_dir,
            ctx.command_max_output_len,
            Some(ctx),
        )
        .map(|output| finish_dispatch_parsed(name, output)),
        "read_file" => read_file_try_dispatch(args_json, ctx)
            .map(|output| finish_dispatch_parsed(name, output)),
        "search_in_files" => grep_try::search_in_files_try(args_json, ctx.working_dir)
            .map(|output| finish_dispatch_parsed(name, output)),
        _ => {
            let output = run_tool(name, args_json, ctx);
            let parsed = crate::tool_result::parse_legacy_output(name, &output);
            if parsed.ok {
                Ok((output, parsed))
            } else {
                Err(ToolError::from_parsed_legacy(name, &parsed, output))
            }
        }
    }
}

fn finish_dispatch_parsed(
    name: &str,
    output: String,
) -> (String, crate::tool_result::ParsedLegacyOutput) {
    let parsed = crate::tool_result::parse_legacy_output(name, &output);
    (output, parsed)
}

/// 与 [`run_tool`] 相同，但失败时返回 [`crate::tool_result::ToolError`]（含 **分类 / 错误码 / retryable**）。
///
/// **`run_command`**、**`cargo_*` / `rust_test_one`**、**`read_file`**、**`search_in_files`** 在 [`run_tool_dispatch`] 中经 `*_try` 返回显式 [`ToolError`]；其余工具仍由 [`crate::tool_result::parse_legacy_output`] 从正文推断。
#[allow(dead_code, clippy::result_large_err)] // 供编排与单测显式 `Result` 分支；主路径现经 [`run_tool_dispatch`] + [`run_tool_result`]
pub fn run_tool_try(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::tool_result::ToolError> {
    run_tool_dispatch(name, args_json, ctx).map(|(output, _)| output)
}

/// 执行本地工具并返回结构化结果（兼容既有字符串输出）。
pub fn run_tool_result(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> ToolResult {
    match run_tool_dispatch(name, args_json, ctx) {
        Ok((output, parsed)) => ToolResult::from_parsed(output, parsed),
        Err(e) => ToolResult::from_parsed(e.message, e.legacy_parsed),
    }
}

/// 判断本次 run_command 是否为“成功的编译命令”（常见 C/C++ 构建工具且退出码为 0）
pub(crate) fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let cmd = v
        .get("command")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_lowercase());
    let is_compile_cmd = cmd.as_deref().is_some_and(|c| {
        matches!(
            c,
            "gcc" | "g++" | "clang" | "clang++" | "make" | "cmake" | "ninja"
        )
    });
    if !is_compile_cmd {
        return false;
    }
    // run_command 输出的第一行形如：退出码：0
    let first_line = result.lines().next().unwrap_or("");
    if let Some(rest) = first_line.strip_prefix("退出码：")
        && let Ok(code) = rest.trim().parse::<i32>()
    {
        return code == 0;
    }
    false
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub(crate) fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    let spec = find_spec(name)?;
    match &spec.summary {
        ToolSummaryKind::None => None,
        ToolSummaryKind::Static(s) => Some((*s).to_string()),
        ToolSummaryKind::Dynamic(f) => {
            let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
            f(&v)
        }
    }
}

/// 使用预解析的 `serde_json::Value` 生成摘要，避免重复 JSON 解析。
pub(crate) fn summarize_tool_call_parsed(
    name: &str,
    args_parsed: &serde_json::Value,
) -> Option<String> {
    let spec = find_spec(name)?;
    match &spec.summary {
        ToolSummaryKind::None => None,
        ToolSummaryKind::Static(s) => Some((*s).to_string()),
        ToolSummaryKind::Dynamic(f) => f(args_parsed),
    }
}

#[cfg(test)]
#[path = "mod/tests.rs"]
mod tests;
