//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod calc;
mod cargo_tools;
mod ci_tools;
mod code_metrics;
mod code_nav;
mod command;
mod date_calc;
mod debug_tools;
mod diagnostics;
pub(crate) use diagnostics::capture_trimmed;
mod env_var_check;
mod error_playbook;
mod exec;
mod file;
pub(crate) use file::canonical_workspace_root;
mod format;
mod frontend_tools;
mod git;
mod go_tools;
mod grep;
pub mod http_fetch;
mod json_format;
mod lint;
mod markdown_links;
mod nodejs_tools;
pub(crate) mod output_util;
mod package_query;
mod patch;
mod precommit_tools;
mod process_tools;
mod python_tools;
mod quality_tools;
mod regex_test;
mod release_docs;
mod rust_ide;
mod schedule;
mod schema_check;
mod security_tools;
mod source_analysis_tools;
mod spell_astgrep_tools;
mod structured_data;
mod symbol;
mod table_text;
mod text_diff;
mod text_transform;
mod time;
mod todo_scan;
mod tool_params;
mod tool_specs_registry;
mod tool_summary;
mod unit_convert;
mod weather;
mod web_search;

pub mod dev_tag;

use crate::config::AgentConfig;
use crate::tool_result::ToolResult;
use crate::types::{FunctionDef, Tool};

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：时间/计算/天气、联网搜索与受控 HTTP、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

pub struct ToolContext<'a> {
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
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
pub fn tool_context_for<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
) -> ToolContext<'a> {
    ToolContext {
        command_max_output_len: cfg.command_max_output_len,
        weather_timeout_secs: cfg.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search_timeout_secs,
        web_search_provider: cfg.web_search_provider,
        web_search_api_key: cfg.web_search_api_key.as_str(),
        web_search_max_results: cfg.web_search_max_results,
        http_fetch_allowed_prefixes: cfg.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch_max_response_bytes,
        read_file_turn_cache: None,
    }
}

/// 与 [`tool_context_for`] 相同，但可挂载单轮 `read_file` 缓存（供 `dispatch_tool` / `execute_tools`）。
pub fn tool_context_for_with_read_cache<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<&'a crate::read_file_turn_cache::ReadFileTurnCache>,
) -> ToolContext<'a> {
    ToolContext {
        read_file_turn_cache,
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
    command::run(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

fn runner_run_executable(args: &str, ctx: &ToolContext<'_>) -> String {
    exec::run(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_package_query(args: &str, ctx: &ToolContext<'_>) -> String {
    package_query::run(args, ctx.command_max_output_len)
}

fn runner_cargo_check(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_check(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_cargo_test(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_test(args, ctx.working_dir, ctx.command_max_output_len)
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
    cargo_tools::rust_test_one(args, ctx.working_dir, ctx.command_max_output_len)
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

fn runner_changelog_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::changelog_draft(args, ctx.working_dir, ctx.command_max_output_len)
}

fn runner_license_notice(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::license_notice(args, ctx.working_dir, ctx.command_max_output_len)
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

fn runner_git_status(args: &str, ctx: &ToolContext<'_>) -> String {
    git::status(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_clean_check(args: &str, ctx: &ToolContext<'_>) -> String {
    git::clean_check(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff_stat(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_stat(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_git_diff_names(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_names(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_log(args: &str, ctx: &ToolContext<'_>) -> String {
    git::log(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_show(args: &str, ctx: &ToolContext<'_>) -> String {
    git::show(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_diff_base(args: &str, ctx: &ToolContext<'_>) -> String {
    git::diff_base(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_blame(args: &str, ctx: &ToolContext<'_>) -> String {
    git::blame(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_file_history(args: &str, ctx: &ToolContext<'_>) -> String {
    git::file_history(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_branch_list(args: &str, ctx: &ToolContext<'_>) -> String {
    git::branch_list(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_status(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_status(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_stage_files(args: &str, ctx: &ToolContext<'_>) -> String {
    git::stage_files(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_commit(args: &str, ctx: &ToolContext<'_>) -> String {
    git::commit(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    git::fetch(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_list(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_list(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_remote_set_url(args: &str, ctx: &ToolContext<'_>) -> String {
    git::remote_set_url(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_apply(args: &str, ctx: &ToolContext<'_>) -> String {
    git::apply(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_clone(args: &str, ctx: &ToolContext<'_>) -> String {
    git::clone_repo(args, ctx.command_max_output_len, ctx.working_dir)
}

fn runner_create_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_file(args, ctx.working_dir)
}

fn runner_modify_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::modify_file(args, ctx.working_dir)
}

fn runner_copy_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::copy_file(args, ctx.working_dir)
}

fn runner_move_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::move_file(args, ctx.working_dir)
}

fn runner_read_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_file(args, ctx.working_dir, ctx)
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
    patch::run(args, ctx.working_dir)
}

fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
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
    structured_data::structured_patch(args, ctx.working_dir)
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

// ── Git 写操作补全 ──────────────────────────────────────────

fn runner_git_checkout(args: &str, ctx: &ToolContext<'_>) -> String {
    git::checkout(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_branch_create(args: &str, ctx: &ToolContext<'_>) -> String {
    git::branch_create(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_branch_delete(args: &str, ctx: &ToolContext<'_>) -> String {
    git::branch_delete(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_push(args: &str, ctx: &ToolContext<'_>) -> String {
    git::push(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_merge(args: &str, ctx: &ToolContext<'_>) -> String {
    git::merge(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_rebase(args: &str, ctx: &ToolContext<'_>) -> String {
    git::rebase(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_stash(args: &str, ctx: &ToolContext<'_>) -> String {
    git::stash(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_tag(args: &str, ctx: &ToolContext<'_>) -> String {
    git::tag(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_reset(args: &str, ctx: &ToolContext<'_>) -> String {
    git::reset(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_cherry_pick(args: &str, ctx: &ToolContext<'_>) -> String {
    git::cherry_pick(args, ctx.command_max_output_len, ctx.working_dir)
}
fn runner_git_revert(args: &str, ctx: &ToolContext<'_>) -> String {
    git::revert(args, ctx.command_max_output_len, ctx.working_dir)
}

// ── Node.js / npm / npx ─────────────────────────────────────

fn runner_npm_install(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_install(args, ctx.working_dir, ctx.command_max_output_len)
}
fn runner_npm_run(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_run(args, ctx.working_dir, ctx.command_max_output_len)
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
    file::delete_file(args, ctx.working_dir)
}
fn runner_delete_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::delete_dir(args, ctx.working_dir)
}
fn runner_append_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::append_file(args, ctx.working_dir)
}
fn runner_create_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_dir(args, ctx.working_dir)
}
fn runner_search_replace(args: &str, ctx: &ToolContext<'_>) -> String {
    file::search_replace(args, ctx.working_dir)
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

/// 执行本地工具并返回结构化结果（兼容既有字符串输出）。
pub fn run_tool_result(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> ToolResult {
    let output = run_tool(name, args_json, ctx);
    ToolResult::from_legacy_output(name, output)
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
