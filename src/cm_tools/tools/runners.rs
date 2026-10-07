//! 内置工具的 `runner_*` 薄封装与 [`ToolSpec`] 类型（由 [`super::tool_specs_registry`] 引用）。
use super::*;

/// 工具执行体的两种签名形态（strangler 迁移期并存；全部工具迁完后删除 [`ToolRunner::Legacy`]）。
///
/// - [`ToolRunner::Legacy`]：返回 `String`，失败状态由 `parse_legacy_output` 从正文推断（历史形态）。
/// - [`ToolRunner::Typed`]：返回 `Result<String, ToolError>`，失败路径显式（与各 `*_try` 函数同签名）；
///   成功侧仍为 `String` 正文，结构化载荷由 `parse_legacy_output` 生成（行为与 Legacy 一致）。
#[derive(Clone, Copy)]
pub enum ToolRunner {
    /// 历史签名：错误状态从正文推断；PR3+ 按族迁移为 [`ToolRunner::Typed`]。
    Legacy(fn(args_json: &str, ctx: &ToolContext<'_>) -> String),
    /// 显式失败签名：`Err(ToolError)` 直接透传给编排层（与 `*_try` 函数一致）。
    #[allow(clippy::result_large_err)]
    Typed(fn(args_json: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError>),
}

pub type ParamBuilder = fn() -> serde_json::Value;

/// 工具调用摘要类型：用于前端 Chat 面板展示。
#[derive(Clone, Copy)]
pub enum ToolSummaryKind {
    /// 无自定义摘要。
    None,
    /// 固定摘要字符串（与参数无关）。
    Static(&'static str),
    /// 从解析后的 args JSON 动态生成摘要。
    Dynamic(fn(&serde_json::Value) -> Option<String>),
}

#[derive(Clone, Copy)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub category: super::ToolCategory,
    pub parameters: ParamBuilder,
    pub runner: ToolRunner,
    pub summary: ToolSummaryKind,
}

#[inline]
pub fn tool_spec_requires_fastembed(name: &str) -> bool {
    name == "codebase_semantic_search"
}

// ── 同构薄封装宏 ────────────────────────────────────────────
//
// 绝大多数 `runner_*` 只是把 `(args, ctx)` 转发给对应实现模块，转发参数由形态决定。
// 这里按调用形态各定义一个宏；新增同类工具时只需在对应表中增一行 `runner_x => module::fn`。
// 注意：形态相近（签名一致）的表若写错配对，编译器无法发现，新增条目时务必核对右侧路径。

/// 形态：`f(args, working_dir, command_max_output_len)`。
macro_rules! define_runners_cwd_maxlen {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
                $func(args, ctx.working_dir, ctx.command_max_output_len)
            }
        )*
    };
}

/// 形态：`f(args, working_dir, command_max_output_len) -> Result<String, ToolError>`（[`ToolRunner::Typed`] 孪生）。
macro_rules! define_runners_cwd_maxlen_try {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            #[allow(clippy::result_large_err)]
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
                $func(args, ctx.working_dir, ctx.command_max_output_len)
            }
        )*
    };
}

/// 形态：`f(args, working_dir, ctx)`。
macro_rules! define_runners_cwd_ctx {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
                $func(args, ctx.working_dir, ctx)
            }
        )*
    };
}

/// 形态：`f(args, working_dir)`。
macro_rules! define_runners_cwd {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
                $func(args, ctx.working_dir)
            }
        )*
    };
}

/// 形态：`f(args)`（不使用 `ctx`）。
macro_rules! define_runners_args_only {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            pub fn $runner(args: &str, _ctx: &ToolContext<'_>) -> String {
                $func(args)
            }
        )*
    };
}

/// 生成 `fn runner_git_* -> git::impl(args, max_len, cwd)`；新增 Git 工具时在列表中增一行并注册 `tool_specs_registry`。
macro_rules! define_git_runner {
    ($runner:ident, $git_fn:ident) => {
        pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
            git::$git_fn(args, ctx.command_max_output_len, ctx.working_dir)
        }
    };
}

macro_rules! define_git_runners {
    ($( $runner:ident => $git_fn:ident ),* $(,)? ) => {
        $( define_git_runner!($runner, $git_fn); )*
    };
}

/// 生成 `fn runner_gh_* -> github_cli::gh_*(args, max_len, allowed, cwd)`；新增 GitHub 工具时在列表中增一行。
macro_rules! gh_runner {
    ($name:ident, $fn:path) => {
        pub fn $name(args: &str, ctx: &ToolContext<'_>) -> String {
            $fn(
                args,
                ctx.command_max_output_len,
                ctx.allowed_commands,
                ctx.working_dir,
            )
        }
    };
}

// ── 需要定制逻辑的 runner（不适用同构宏）─────────────────────

pub fn runner_get_current_time(args: &str, _ctx: &ToolContext<'_>) -> String {
    let parsed: super::tool_param_types::GetCurrentTimeArgs =
        super::parse_args_typed(args).unwrap_or_default();
    let mode = parsed
        .mode
        .map(super::tool_param_types::GetCurrentTimeMode::to_time_output)
        .unwrap_or(time::TimeOutputMode::Time);
    time::run(mode, parsed.year, parsed.month)
}

pub fn runner_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    let parsed: super::tool_param_types::CalcArgs = match super::parse_args_typed(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    calc::run(&parsed.expression)
}

pub fn runner_run_command(args: &str, ctx: &ToolContext<'_>) -> String {
    let test_cache = ctx
        .test_result_cache_enabled
        .then_some(command::RunCommandTestCacheOpts {
            enabled: true,
            max_entries: ctx.test_result_cache_max_entries,
            workspace_root: ctx.working_dir,
        });
    command::run_with_wait(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
        test_cache,
        false,
        &crate::cm_tools::subprocess_session::SubprocessWaitCtl::with_wall_secs(
            ctx.command_timeout_secs,
        ),
    )
}

pub fn runner_terminal_session(args: &str, _ctx: &ToolContext<'_>) -> String {
    let _ = args;
    "错误：terminal_session 须由服务端异步调度执行（不走同步 run_tool）。".to_string()
}

pub fn runner_workflow_execute(_args: &str, _ctx: &ToolContext<'_>) -> String {
    // 由 runtime 在 run_agent_turn 中拦截实际执行。
    "workflow_execute：由运行时引擎执行（若你看到这条，说明拦截未生效）。".to_string()
}

/// `cargo_check` 的 Typed runner：显式 `ToolError`（试点归并，原 dispatch 特判移入 spec）。
#[allow(clippy::result_large_err)]
pub fn runner_cargo_check_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    cargo_tools::cargo_check_try(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── 原 dispatch 特判的 cargo_* / rust_* Typed runner（strangler 收尾）──

define_runners_cwd_maxlen_try! {
    runner_cargo_clippy_try => cargo_tools::cargo_clippy_try,
    runner_cargo_run_try => cargo_tools::cargo_run_try,
    runner_rust_rustc_try => cargo_tools::rust_rustc_try,
}

/// `cargo_test` 的 Typed runner：额外透传 `ctx`（测试结果缓存 / 超时）。
#[allow(clippy::result_large_err)]
pub fn runner_cargo_test_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    cargo_tools::cargo_test_try(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

/// `rust_test_one` 的 Typed runner：额外透传 `ctx`。
#[allow(clippy::result_large_err)]
pub fn runner_rust_test_one_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    cargo_tools::rust_test_one_try(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

pub fn runner_pytest_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::pytest_run(
        args,
        ctx.working_dir,
        ctx.command_max_output_len,
        Some(ctx.command_timeout_secs),
    )
}

pub fn runner_python_snippet_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::python_snippet_run(
        args,
        ctx.working_dir,
        ctx.command_max_output_len,
        ctx.command_timeout_secs,
    )
}

pub fn runner_diagnostic_summary(args: &str, ctx: &ToolContext<'_>) -> String {
    diagnostics::diagnostic_summary(args, ctx.working_dir, &[])
}

pub fn runner_self_config_info(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let parsed: super::tool_param_types::SelfConfigInfoArgs = match super::parse_args_typed(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let out = self_config_info::self_config_info(cfg, parsed.sections.as_deref());
    if out.is_empty() {
        return format!(
            "未匹配到任何配置小节；可用小节：{}。",
            self_config_info::SECTION_NAMES.join("、")
        );
    }
    out
}

pub fn runner_skill_manage(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let parsed: super::tool_param_types::SkillManageArgs = match super::parse_args_typed(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    skill_manage::skill_manage(parsed, &cfg.skills, ctx.working_dir)
}

pub fn runner_apply_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    patch::run_with_changelist(args, ctx.working_dir, ctx.workspace_changelist)
}

pub fn runner_codebase_semantic_search(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(host) = ctx.codebase_semantic_host else {
        return "错误：当前执行环境未注入代码语义检索配置，无法使用 codebase_semantic_search（如部分工作流节点路径）"
            .to_string();
    };
    host.run_search(args, ctx.working_dir, ctx.command_max_output_len)
}

fn dispatch_long_term_tool(name: &str, args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(cfg) = ctx.cfg else {
        return "错误：工具上下文缺少 AgentConfig".to_string();
    };
    let Some(host) = ctx.long_term_memory_host else {
        return "错误：当前会话未挂载长期记忆运行时（或未启用持久化）".to_string();
    };
    host.dispatch(name, args, cfg)
}

pub fn runner_long_term_remember(args: &str, ctx: &ToolContext<'_>) -> String {
    dispatch_long_term_tool("long_term_remember", args, ctx)
}

pub fn runner_summarize_experience(args: &str, ctx: &ToolContext<'_>) -> String {
    dispatch_long_term_tool("summarize_experience", args, ctx)
}

pub fn runner_long_term_forget(args: &str, ctx: &ToolContext<'_>) -> String {
    dispatch_long_term_tool("long_term_forget", args, ctx)
}

pub fn runner_long_term_memory_list(args: &str, ctx: &ToolContext<'_>) -> String {
    dispatch_long_term_tool("long_term_memory_list", args, ctx)
}

pub fn runner_background_job_status(args: &str, ctx: &ToolContext<'_>) -> String {
    background_job_tools::background_job_status(
        args,
        ctx.tool_jobs_host,
        ctx.command_max_output_len,
    )
}

pub fn runner_background_job_list(args: &str, ctx: &ToolContext<'_>) -> String {
    background_job_tools::background_job_list(
        args,
        ctx.tool_jobs_host,
        ctx.working_dir,
        ctx.command_max_output_len,
    )
}

pub fn runner_background_job_output(args: &str, ctx: &ToolContext<'_>) -> String {
    background_job_tools::background_job_output(
        args,
        ctx.tool_jobs_host,
        ctx.command_max_output_len,
    )
}

pub fn runner_background_job_cancel(args: &str, ctx: &ToolContext<'_>) -> String {
    background_job_tools::background_job_cancel(args, ctx.tool_jobs_host)
}

#[allow(clippy::result_large_err)]
pub fn read_file_try_dispatch(
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::cm_tools::tool_result::ToolError> {
    file::read_file_try(args_json, ctx.working_dir, ctx)
}

/// 用户消息 `@路径` 展开等：与 `read_file` 工具同源校验与读取。
#[allow(clippy::result_large_err)]
pub fn read_file_try_at_paths(
    args_json: &str,
    working_dir: &std::path::Path,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::cm_tools::tool_result::ToolError> {
    file::read_file_try(args_json, working_dir, ctx)
}

/// `search_in_files` 的 Typed runner：显式 `ToolError`（试点归并，原 dispatch 特判移入 spec）。
#[allow(clippy::result_large_err)]
pub fn runner_search_in_files_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    grep_try::search_in_files_try(args, ctx.working_dir)
}

// ── 转发参数较少、单独列出更直观的 runner ───────────────────

pub fn runner_get_weather(args: &str, ctx: &ToolContext<'_>) -> String {
    weather::run(args, ctx.weather_timeout_secs)
}

pub fn runner_web_search(args: &str, ctx: &ToolContext<'_>) -> String {
    web_search::run(args, ctx)
}

pub fn runner_http_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_direct(args, ctx)
}

pub fn runner_http_request(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_request_direct(args, ctx)
}

pub fn runner_playbook_run_commands(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::playbook_run_commands(args, ctx)
}

pub fn runner_error_output_playbook(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::error_output_playbook(args, ctx.allowed_commands)
}

pub fn runner_crate_contract_map(args: &str, ctx: &ToolContext<'_>) -> String {
    contract_map::crate_contract_map(args, ctx)
}

pub fn runner_package_query(args: &str, ctx: &ToolContext<'_>) -> String {
    package_query::run(args, ctx.command_max_output_len)
}

pub fn runner_port_check(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::port_check(args, ctx.command_max_output_len)
}

pub fn runner_process_list(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::process_list(args, ctx.command_max_output_len)
}

// ── 同构薄封装实例（形态 1：f(args, cwd, max_len)）────────────

define_runners_cwd_maxlen! {
    runner_rust_compiler_json => rust_ide::rust_compiler_json,
    runner_ruff_check => python_tools::ruff_check,
    runner_mypy_check => python_tools::mypy_check,
    runner_uv_sync => python_tools::uv_sync,
    runner_uv_run => python_tools::uv_run,
    runner_go_build => go_tools::go_build,
    runner_go_test => go_tools::go_test,
    runner_go_vet => go_tools::go_vet,
    runner_go_fmt_check => go_tools::go_fmt_check,
    runner_golangci_lint => go_tools::golangci_lint,
    runner_pre_commit_run => precommit_tools::pre_commit_run,
    runner_typos_check => spell_astgrep_tools::typos_check,
    runner_codespell_check => spell_astgrep_tools::codespell_check,
    runner_ast_grep_run => spell_astgrep_tools::ast_grep_run,
    runner_ast_grep_rewrite => spell_astgrep_tools::ast_grep_rewrite,
    runner_changelog_draft => release_docs::changelog_draft,
    runner_license_notice => release_docs::license_notice,
    runner_repo_overview_sweep => repo_overview::repo_overview_sweep,
    runner_docs_health_sweep => docs_health_sweep::docs_health_sweep,
    runner_ci_pipeline_local => ci_tools::ci_pipeline_local,
    runner_release_ready_check => ci_tools::release_ready_check,
    runner_run_lints => lint::run,
    runner_quality_workspace => quality_tools::quality_workspace,
    runner_code_stats => code_metrics::code_stats,
    runner_dependency_graph => code_metrics::dependency_graph,
    runner_coverage_report => code_metrics::coverage_report,
}

// ── 同构薄封装实例（形态 2：f(args, cwd, ctx)）────────────────

define_runners_cwd_ctx! {
    runner_archive_pack => archive::archive_pack,
    runner_archive_unpack => archive::archive_unpack,
    runner_archive_list => archive::archive_list,
    runner_create_file => file::create_file,
    runner_modify_file => file::modify_file,
    runner_copy_file => file::copy_file,
    runner_move_file => file::move_file,
    runner_structured_patch => structured_data::structured_patch,
    runner_delete_files => file::delete_files,
    runner_append_file => file::append_file,
    runner_search_replace => file::search_replace,
}

// ── 同构薄封装实例（形态 3：f(args, cwd)）────────────────────

define_runners_cwd! {
    runner_rust_analyzer_goto_definition => rust_ide::rust_analyzer_goto_definition,
    runner_rust_analyzer_find_references => rust_ide::rust_analyzer_find_references,
    runner_rust_analyzer_hover => rust_ide::rust_analyzer_hover,
    runner_rust_analyzer_document_symbol => rust_ide::rust_analyzer_document_symbol,
    runner_rust_analyzer_goto_implementation => rust_ide::rust_analyzer_goto_implementation,
    runner_rust_analyzer_goto_type_definition => rust_ide::rust_analyzer_goto_type_definition,
    runner_rust_analyzer_document_highlight => rust_ide::rust_analyzer_document_highlight,
    runner_rust_analyzer_workspace_symbol => rust_ide::rust_analyzer_workspace_symbol,
    runner_read_dir => file::read_dir,
    runner_glob_files => file::glob_files,
    runner_list_tree => file::list_tree,
    runner_file_exists => file::file_exists,
    runner_read_binary_meta => file::read_binary_meta,
    runner_hash_file => file::hash_file,
    runner_extract_in_file => file::extract_in_file,
    runner_markdown_check_links => markdown_links::markdown_check_links,
    runner_structured_validate => structured_data::structured_validate,
    runner_structured_query => structured_data::structured_query,
    runner_structured_diff => structured_data::structured_diff,
    runner_text_diff => text_diff::run,
    runner_table_text => table_text::run,
    runner_find_symbol => symbol::run,
    runner_find_references => code_nav::find_references,
    runner_rust_file_outline => code_nav::rust_file_outline,
    runner_call_graph_sketch => call_graph_sketch::run,
    runner_format_file => format::run,
    runner_format_check_file => format::run_check,
    runner_add_reminder => schedule::add_reminder,
    runner_list_reminders => schedule::list_reminders,
    runner_complete_reminder => schedule::complete_reminder,
    runner_delete_reminder => schedule::delete_reminder,
    runner_update_reminder => schedule::update_reminder,
    runner_add_event => schedule::add_event,
    runner_list_events => schedule::list_events,
    runner_delete_event => schedule::delete_event,
    runner_update_event => schedule::update_event,
    runner_delete_dir => file::delete_dir,
    runner_create_dir => file::create_dir,
    runner_chmod_file => file::chmod_file,
    runner_symlink_info => file::symlink_info,
    runner_todo_scan => todo_scan::run,
}

// ── 同构薄封装实例（形态 4：f(args)）─────────────────────────

define_runners_args_only! {
    runner_convert_units => unit_convert::run,
    runner_backtrace_analyze => debug_tools::rust_backtrace_analyze,
    runner_present_clarification_questionnaire => crate::cm_tools::clarification_questionnaire::run_present_clarification_questionnaire,
    runner_text_transform => text_transform::run,
    runner_regex_test => regex_test::run,
    runner_date_calc => date_calc::run,
    runner_json_format => json_format::run,
    runner_env_var_check => env_var_check::run,
}

// ── Git 工具 ────────────────────────────────────────────────

define_git_runners! {
    runner_git_status => status,
    runner_git_diff => diff,
    runner_git_clean_check => clean_check,
    runner_git_log => log,
    runner_git_show => show,
    runner_git_blame => blame,
    runner_git_file_history => file_history,
    runner_git_branch_list => branch_list,
    runner_git_remote_status => remote_status,
    runner_git_remote_list => remote_list,
}

// ── GitHub CLI 工具 ─────────────────────────────────────────

gh_runner!(runner_gh_pr_list, github_cli::gh_pr_list);
gh_runner!(runner_gh_pr_view, github_cli::gh_pr_view);
gh_runner!(runner_gh_pr_checks, github_cli::gh_pr_checks);
gh_runner!(runner_gh_pr_create, github_cli::gh_pr_create);
gh_runner!(runner_gh_pr_merge, github_cli::gh_pr_merge);
gh_runner!(runner_gh_pr_review, github_cli::gh_pr_review);
gh_runner!(runner_gh_pr_comment, github_cli::gh_pr_comment);
gh_runner!(runner_gh_pr_edit, github_cli::gh_pr_edit);
gh_runner!(runner_gh_issue_list, github_cli::gh_issue_list);
gh_runner!(runner_gh_issue_view, github_cli::gh_issue_view);
gh_runner!(runner_gh_issue_create, github_cli::gh_issue_create);
gh_runner!(runner_gh_run_list, github_cli::gh_run_list);
gh_runner!(runner_gh_pr_diff, github_cli::gh_pr_diff);
gh_runner!(runner_gh_run_view, github_cli::gh_run_view);
gh_runner!(runner_gh_run_rerun, github_cli::gh_run_rerun);
gh_runner!(
    runner_gh_run_failure_summary,
    github_cli::gh_run_failure_summary
);
gh_runner!(runner_gh_release_list, github_cli::gh_release_list);
gh_runner!(runner_gh_release_view, github_cli::gh_release_view);
gh_runner!(runner_gh_release_create, github_cli::gh_release_create);
gh_runner!(runner_gh_search, github_cli::gh_search);
gh_runner!(runner_gh_api, github_cli::gh_api);

pub fn runner_gh_pr_body_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_body_draft(args, ctx.working_dir, ctx.command_max_output_len)
}
