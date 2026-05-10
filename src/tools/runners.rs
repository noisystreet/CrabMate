//! 内置工具的 `runner_*` 薄封装与 [`ToolSpec`] 类型别名（由 [`super::tool_specs_registry`] 引用）。
use super::*;

pub(crate) type ToolRunner = fn(args_json: &str, ctx: &super::ToolContext<'_>) -> String;
pub(crate) type ParamBuilder = fn() -> serde_json::Value;

/// 工具调用摘要类型：用于前端 Chat 面板展示。
#[derive(Clone, Copy)]
pub(crate) enum ToolSummaryKind {
    /// 无自定义摘要。
    None,
    /// 固定摘要字符串（与参数无关）。
    Static(&'static str),
    /// 从解析后的 args JSON 动态生成摘要。
    Dynamic(fn(&serde_json::Value) -> Option<String>),
}

#[derive(Clone, Copy)]
pub(crate) struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub category: super::ToolCategory,
    pub parameters: ParamBuilder,
    pub runner: ToolRunner,
    pub summary: ToolSummaryKind,
}

#[inline]
pub(crate) fn tool_spec_requires_fastembed(name: &str) -> bool {
    name == "codebase_semantic_search"
}

pub(crate) fn runner_get_current_time(args: &str, _ctx: &ToolContext<'_>) -> String {
    let parsed: super::tool_param_types::GetCurrentTimeArgs =
        serde_json::from_str(args).unwrap_or_default();
    let mode = parsed
        .mode
        .map(super::tool_param_types::GetCurrentTimeMode::to_time_output)
        .unwrap_or(time::TimeOutputMode::Time);
    time::run(mode, parsed.year, parsed.month)
}

pub(crate) fn runner_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    let parsed: super::tool_param_types::CalcArgs = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(e) => return format!("错误：参数 JSON 无效: {e}"),
    };
    calc::run(&parsed.expression)
}

pub(crate) fn runner_convert_units(args: &str, _ctx: &ToolContext<'_>) -> String {
    unit_convert::run(args)
}

pub(crate) fn runner_get_weather(args: &str, ctx: &ToolContext<'_>) -> String {
    weather::run(args, ctx.weather_timeout_secs)
}

pub(crate) fn runner_web_search(args: &str, ctx: &ToolContext<'_>) -> String {
    web_search::run(args, ctx)
}

pub(crate) fn runner_http_fetch(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_direct(args, ctx)
}

pub(crate) fn runner_http_request(args: &str, ctx: &ToolContext<'_>) -> String {
    http_fetch::run_request_direct(args, ctx)
}

pub(crate) fn runner_terminal_session(args: &str, _ctx: &ToolContext<'_>) -> String {
    let _ = args;
    "错误：terminal_session 须由服务端异步调度执行（不走同步 run_tool）。".to_string()
}

pub(crate) fn runner_run_command(args: &str, ctx: &ToolContext<'_>) -> String {
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

pub(crate) fn runner_package_query(args: &str, ctx: &ToolContext<'_>) -> String {
    package_query::run(args, ctx.command_max_output_len)
}

pub(crate) fn runner_gh_pr_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_pr_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_pr_checks(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_checks(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_pr_create(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_create(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_issue_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_issue_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_issue_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_issue_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_run_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_run_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_pr_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_pr_diff(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_run_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_run_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_release_list(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_release_list(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_release_view(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_release_view(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_search(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_search(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_gh_api(args: &str, ctx: &ToolContext<'_>) -> String {
    github_cli::gh_api(
        args,
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}

pub(crate) fn runner_cargo_check(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_test(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_test(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

pub(crate) fn runner_cargo_clippy(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clippy(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_metadata(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_metadata(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cargo_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_tree(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cargo_clean(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_clean(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cargo_doc(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_doc(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cargo_nextest(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_nextest(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_fmt_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::cargo_fmt_check_tool(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_outdated(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_outdated(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_machete(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_machete(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_udeps(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_udeps(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_publish_dry_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_publish_dry_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_rust_compiler_json(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_compiler_json(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_rust_rustc(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::rust_rustc(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_rust_analyzer_goto_definition(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_goto_definition(args, ctx.working_dir)
}

pub(crate) fn runner_rust_analyzer_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_find_references(args, ctx.working_dir)
}

pub(crate) fn runner_rust_analyzer_hover(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_hover(args, ctx.working_dir)
}

pub(crate) fn runner_rust_analyzer_document_symbol(args: &str, ctx: &ToolContext<'_>) -> String {
    rust_ide::rust_analyzer_document_symbol(args, ctx.working_dir)
}

pub(crate) fn runner_cargo_fix(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_fix(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_run(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::cargo_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_rust_test_one(args: &str, ctx: &ToolContext<'_>) -> String {
    cargo_tools::rust_test_one(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
}

pub(crate) fn runner_ruff_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::ruff_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_pytest_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::pytest_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_mypy_check(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::mypy_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_python_install_editable(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::python_install_editable(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_uv_sync(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_sync(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_uv_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::uv_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_python_snippet_run(args: &str, ctx: &ToolContext<'_>) -> String {
    python_tools::python_snippet_run(
        args,
        ctx.working_dir,
        ctx.command_max_output_len,
        ctx.command_timeout_secs,
    )
}

pub(crate) fn runner_go_build(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_build(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_go_test(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_test(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_go_vet(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_vet(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_go_mod_tidy(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_mod_tidy(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_go_fmt_check(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::go_fmt_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_maven_compile(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::maven_compile(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_maven_test(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::maven_test(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_gradle_compile(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::gradle_compile(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_gradle_test(args: &str, ctx: &ToolContext<'_>) -> String {
    jvm_tools::gradle_test(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_docker_build(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::docker_build(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_docker_compose_ps(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::docker_compose_ps(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_podman_images(args: &str, ctx: &ToolContext<'_>) -> String {
    container_tools::podman_images(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_pre_commit_run(args: &str, ctx: &ToolContext<'_>) -> String {
    precommit_tools::pre_commit_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_typos_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::typos_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_codespell_check(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::codespell_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_ast_grep_run(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_ast_grep_rewrite(args: &str, ctx: &ToolContext<'_>) -> String {
    spell_astgrep_tools::ast_grep_rewrite(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_frontend_lint(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_lint(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_frontend_build(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_build(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_frontend_test(args: &str, ctx: &ToolContext<'_>) -> String {
    frontend_tools::frontend_test(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_cargo_audit(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_audit(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cargo_deny(args: &str, ctx: &ToolContext<'_>) -> String {
    security_tools::cargo_deny(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_backtrace_analyze(args: &str, _ctx: &ToolContext<'_>) -> String {
    debug_tools::rust_backtrace_analyze(args)
}

pub(crate) fn runner_diagnostic_summary(args: &str, ctx: &ToolContext<'_>) -> String {
    diagnostics::diagnostic_summary(args, ctx.working_dir)
}

pub(crate) fn runner_present_clarification_questionnaire(
    args: &str,
    _ctx: &ToolContext<'_>,
) -> String {
    crate::clarification_questionnaire::run_present_clarification_questionnaire(args)
}

pub(crate) fn runner_long_term_remember(args: &str, ctx: &ToolContext<'_>) -> String {
    long_term_memory_tools::long_term_remember(args, ctx)
}

pub(crate) fn runner_summarize_experience(args: &str, ctx: &ToolContext<'_>) -> String {
    long_term_memory_tools::summarize_experience(args, ctx)
}

pub(crate) fn runner_long_term_forget(args: &str, ctx: &ToolContext<'_>) -> String {
    long_term_memory_tools::long_term_forget(args, ctx)
}

pub(crate) fn runner_long_term_memory_list(args: &str, ctx: &ToolContext<'_>) -> String {
    long_term_memory_tools::long_term_memory_list(args, ctx)
}

pub(crate) fn runner_error_output_playbook(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::error_output_playbook(args, ctx.allowed_commands)
}

pub(crate) fn runner_playbook_run_commands(args: &str, ctx: &ToolContext<'_>) -> String {
    error_playbook::playbook_run_commands(args, ctx)
}

pub(crate) fn runner_changelog_draft(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::changelog_draft(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_license_notice(args: &str, ctx: &ToolContext<'_>) -> String {
    release_docs::license_notice(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_repo_overview_sweep(args: &str, ctx: &ToolContext<'_>) -> String {
    repo_overview::repo_overview_sweep(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_crate_contract_map(args: &str, ctx: &ToolContext<'_>) -> String {
    contract_map::crate_contract_map(args, ctx)
}

pub(crate) fn runner_docs_health_sweep(args: &str, ctx: &ToolContext<'_>) -> String {
    docs_health_sweep::docs_health_sweep(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_ci_pipeline_local(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::ci_pipeline_local(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_release_ready_check(args: &str, ctx: &ToolContext<'_>) -> String {
    ci_tools::release_ready_check(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_workflow_execute(_args: &str, _ctx: &ToolContext<'_>) -> String {
    // 由 runtime 在 run_agent_turn 中拦截实际执行。
    "workflow_execute：由运行时引擎执行（若你看到这条，说明拦截未生效）。".to_string()
}

/// 生成 `fn runner_git_* -> git::impl(args, max_len, cwd)`；新增 Git 工具时在列表中增一行并注册 `tool_specs_registry`。
macro_rules! define_git_runner {
    ($runner:ident, $git_fn:ident) => {
        pub(crate) fn $runner(args: &str, ctx: &ToolContext<'_>) -> String {
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

pub(crate) fn runner_archive_pack(args: &str, ctx: &ToolContext<'_>) -> String {
    archive::archive_pack(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_archive_unpack(args: &str, ctx: &ToolContext<'_>) -> String {
    archive::archive_unpack(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_archive_list(args: &str, ctx: &ToolContext<'_>) -> String {
    archive::archive_list(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_create_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_file(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_modify_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::modify_file(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_copy_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::copy_file(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_move_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::move_file(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_read_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_file(args, ctx.working_dir, ctx)
}

#[allow(clippy::result_large_err)]
pub(crate) fn read_file_try_dispatch(
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::tool_result::ToolError> {
    file::read_file_try(args_json, ctx.working_dir, ctx)
}

/// 用户消息 `@路径` 展开等：与 `read_file` 工具同源校验与读取。
#[allow(clippy::result_large_err)]
pub(crate) fn read_file_try_at_paths(
    args_json: &str,
    working_dir: &std::path::Path,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::tool_result::ToolError> {
    file::read_file_try(args_json, working_dir, ctx)
}

pub(crate) fn runner_read_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_dir(args, ctx.working_dir)
}

pub(crate) fn runner_glob_files(args: &str, ctx: &ToolContext<'_>) -> String {
    file::glob_files(args, ctx.working_dir)
}

pub(crate) fn runner_list_tree(args: &str, ctx: &ToolContext<'_>) -> String {
    file::list_tree(args, ctx.working_dir)
}

pub(crate) fn runner_file_exists(args: &str, ctx: &ToolContext<'_>) -> String {
    file::file_exists(args, ctx.working_dir)
}

pub(crate) fn runner_read_binary_meta(args: &str, ctx: &ToolContext<'_>) -> String {
    file::read_binary_meta(args, ctx.working_dir)
}

pub(crate) fn runner_hash_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::hash_file(args, ctx.working_dir)
}

pub(crate) fn runner_extract_in_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::extract_in_file(args, ctx.working_dir)
}

pub(crate) fn runner_apply_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    patch::run_with_changelist(args, ctx.working_dir, ctx.workspace_changelist)
}

pub(crate) fn runner_search_in_files(args: &str, ctx: &ToolContext<'_>) -> String {
    grep::run(args, ctx.working_dir)
}

pub(crate) fn runner_codebase_semantic_search(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(p) = ctx.codebase_semantic.as_ref() else {
        return "错误：当前执行环境未注入代码语义检索配置，无法使用 codebase_semantic_search（如部分工作流节点路径）"
            .to_string();
    };
    crate::memory::codebase_semantic_index::run_tool(
        args,
        ctx.working_dir,
        p,
        ctx.command_max_output_len,
    )
}

pub(crate) fn runner_markdown_check_links(args: &str, ctx: &ToolContext<'_>) -> String {
    markdown_links::markdown_check_links(args, ctx.working_dir)
}

pub(crate) fn runner_structured_validate(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_validate(args, ctx.working_dir)
}

pub(crate) fn runner_structured_query(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_query(args, ctx.working_dir)
}

pub(crate) fn runner_structured_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_diff(args, ctx.working_dir)
}

pub(crate) fn runner_structured_patch(args: &str, ctx: &ToolContext<'_>) -> String {
    structured_data::structured_patch(args, ctx.working_dir, ctx)
}

pub(crate) fn runner_text_transform(args: &str, _ctx: &ToolContext<'_>) -> String {
    text_transform::run(args)
}

pub(crate) fn runner_text_diff(args: &str, ctx: &ToolContext<'_>) -> String {
    text_diff::run(args, ctx.working_dir)
}

pub(crate) fn runner_table_text(args: &str, ctx: &ToolContext<'_>) -> String {
    table_text::run(args, ctx.working_dir)
}

pub(crate) fn runner_find_symbol(args: &str, ctx: &ToolContext<'_>) -> String {
    symbol::run(args, ctx.working_dir)
}

pub(crate) fn runner_find_references(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::find_references(args, ctx.working_dir)
}

pub(crate) fn runner_rust_file_outline(args: &str, ctx: &ToolContext<'_>) -> String {
    code_nav::rust_file_outline(args, ctx.working_dir)
}

pub(crate) fn runner_call_graph_sketch(args: &str, ctx: &ToolContext<'_>) -> String {
    call_graph_sketch::run(args, ctx.working_dir)
}

pub(crate) fn runner_format_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run(args, ctx.working_dir)
}

pub(crate) fn runner_format_check_file(args: &str, ctx: &ToolContext<'_>) -> String {
    format::run_check(args, ctx.working_dir)
}

pub(crate) fn runner_run_lints(args: &str, ctx: &ToolContext<'_>) -> String {
    lint::run(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_quality_workspace(args: &str, ctx: &ToolContext<'_>) -> String {
    quality_tools::quality_workspace(args, ctx.working_dir, ctx.command_max_output_len)
}

pub(crate) fn runner_add_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_reminder(args, ctx.working_dir)
}

pub(crate) fn runner_list_reminders(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_reminders(args, ctx.working_dir)
}

pub(crate) fn runner_complete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::complete_reminder(args, ctx.working_dir)
}

pub(crate) fn runner_delete_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_reminder(args, ctx.working_dir)
}

pub(crate) fn runner_update_reminder(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_reminder(args, ctx.working_dir)
}

pub(crate) fn runner_add_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::add_event(args, ctx.working_dir)
}

pub(crate) fn runner_list_events(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::list_events(args, ctx.working_dir)
}

pub(crate) fn runner_delete_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::delete_event(args, ctx.working_dir)
}

pub(crate) fn runner_update_event(args: &str, ctx: &ToolContext<'_>) -> String {
    schedule::update_event(args, ctx.working_dir)
}

// ── Node.js / npm / npx ─────────────────────────────────────

pub(crate) fn runner_npm_install(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_install(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_npm_run(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npm_run(args, ctx.working_dir, ctx.command_max_output_len, ctx)
}
pub(crate) fn runner_npx_run(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::npx_run(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_tsc_check(args: &str, ctx: &ToolContext<'_>) -> String {
    nodejs_tools::tsc_check(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── Go 补充：golangci-lint ──────────────────────────────────

pub(crate) fn runner_golangci_lint(args: &str, ctx: &ToolContext<'_>) -> String {
    go_tools::golangci_lint(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── 进程与端口管理 ──────────────────────────────────────────

pub(crate) fn runner_port_check(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::port_check(args, ctx.command_max_output_len)
}
pub(crate) fn runner_process_list(args: &str, ctx: &ToolContext<'_>) -> String {
    process_tools::process_list(args, ctx.command_max_output_len)
}

// ── 代码度量与分析 ──────────────────────────────────────────

// ── 文件增强 ────────────────────────────────────────────────

pub(crate) fn runner_delete_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::delete_file(args, ctx.working_dir, ctx)
}
pub(crate) fn runner_delete_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::delete_dir(args, ctx.working_dir)
}
pub(crate) fn runner_append_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::append_file(args, ctx.working_dir, ctx)
}
pub(crate) fn runner_create_dir(args: &str, ctx: &ToolContext<'_>) -> String {
    file::create_dir(args, ctx.working_dir)
}
pub(crate) fn runner_search_replace(args: &str, ctx: &ToolContext<'_>) -> String {
    file::search_replace(args, ctx.working_dir, ctx)
}
pub(crate) fn runner_chmod_file(args: &str, ctx: &ToolContext<'_>) -> String {
    file::chmod_file(args, ctx.working_dir)
}
pub(crate) fn runner_symlink_info(args: &str, ctx: &ToolContext<'_>) -> String {
    file::symlink_info(args, ctx.working_dir)
}

pub(crate) fn runner_code_stats(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::code_stats(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_dependency_graph(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::dependency_graph(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_coverage_report(args: &str, ctx: &ToolContext<'_>) -> String {
    code_metrics::coverage_report(args, ctx.working_dir, ctx.command_max_output_len)
}

// ── 新增纯内存 / 开发辅助工具 ────────────────────────────────

pub(crate) fn runner_regex_test(args: &str, _ctx: &ToolContext<'_>) -> String {
    regex_test::run(args)
}

pub(crate) fn runner_date_calc(args: &str, _ctx: &ToolContext<'_>) -> String {
    date_calc::run(args)
}

pub(crate) fn runner_json_format(args: &str, _ctx: &ToolContext<'_>) -> String {
    json_format::run(args)
}

pub(crate) fn runner_env_var_check(args: &str, _ctx: &ToolContext<'_>) -> String {
    env_var_check::run(args)
}

pub(crate) fn runner_todo_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    todo_scan::run(args, ctx.working_dir)
}

// ── 源码分析工具 ──────────────────────────────────────────────

pub(crate) fn runner_shellcheck_check(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::shellcheck_check(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_cppcheck_analyze(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::cppcheck_analyze(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_semgrep_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::semgrep_scan(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_hadolint_check(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::hadolint_check(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_bandit_scan(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::bandit_scan(args, ctx.working_dir, ctx.command_max_output_len)
}
pub(crate) fn runner_lizard_complexity(args: &str, ctx: &ToolContext<'_>) -> String {
    source_analysis_tools::lizard_complexity(args, ctx.working_dir, ctx.command_max_output_len)
}
