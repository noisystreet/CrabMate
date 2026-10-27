//! 内置工具的 `runner_*` 薄封装与 [`ToolSpec`] 类型（由 [`super::tool_specs_registry`] 引用）。
use super::*;

/// 工具执行体的签名形态（strangler 迁移期并存；全部工具迁完后删除 [`ToolRunner::Legacy`]）。
///
/// - [`ToolRunner::Legacy`]：返回 `String`，失败状态由 `parse_legacy_output` 从正文推断（历史形态）。
/// - [`ToolRunner::Typed`]：返回 `Result<String, ToolError>`，失败路径显式（与各 `*_try` 函数同签名）；
///   `Ok` 即成功，正文只用于抽取 `exit_code/stdout/stderr`（不再按正文关键词判定失败）。
/// - [`ToolRunner::AsyncOnly`]：仅作 spec 注册占位（工具名 / 参数 schema / 摘要仍可用），
///   同步面没有执行体，须由服务端异步调度执行（`run_command` 经
///   [`super::run_tool_async`] / [`super::run_tool_result_async`]；`terminal_session`
///   经内部 tool_registry）。
#[derive(Clone, Copy)]
pub enum ToolRunner {
    /// 历史签名：错误状态从正文推断；PR3+ 按族迁移为 [`ToolRunner::Typed`]。
    Legacy(fn(args_json: &str, ctx: &ToolContext<'_>) -> String),
    /// 显式失败签名：`Err(ToolError)` 直接透传给编排层（与 `*_try` 函数一致）。
    #[allow(clippy::result_large_err)]
    Typed(fn(args_json: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError>),
    /// 仅异步可执行：同步 `run_tool` 面没有执行体。
    AsyncOnly,
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

/// 形态：`f(args, working_dir, ctx) -> Result<String, ToolError>`（[`ToolRunner::Typed`] 孪生）。
macro_rules! define_runners_cwd_ctx_try {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            #[allow(clippy::result_large_err)]
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
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

/// 形态：`f(args, working_dir) -> Result<String, ToolError>`（[`ToolRunner::Typed`] 孪生）。
macro_rules! define_runners_cwd_try {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            #[allow(clippy::result_large_err)]
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
                $func(args, ctx.working_dir)
            }
        )*
    };
}

/// 形态：`f(args, &PathAccess) -> Result<String, ToolError>`（[`ToolRunner::Typed`] 孪生）。
///
/// 与 [`define_runners_cwd_try`] 的差别只在第二参类型：文件类工具走 [`file::PathAccess`]，
/// 使路径范围（未来 `PathScope`）只需在本宏的构造点处注入，不必逐个工具传导。
/// 新增文件类 `_try` 工具时加入本组，而不是 `define_runners_cwd_try!`。
macro_rules! define_runners_paths_try {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            #[allow(clippy::result_large_err)]
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
                $func(
                    args,
                    &file::PathAccess::with_scope(ctx.working_dir, ctx.path_scope),
                )
            }
        )*
    };
}

/// 形态：`f(args, &PathAccess, ctx) -> Result<String, ToolError>`（[`ToolRunner::Typed`] 孪生）。
///
/// 与 [`define_runners_paths_try`] 的差别只在多带一个 `ctx`（写盘类工具需要变更集/预览上下文）。
macro_rules! define_runners_paths_ctx_try {
    ($( $runner:ident => $func:path ),* $(,)?) => {
        $(
            #[allow(clippy::result_large_err)]
            pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
                $func(
                    args,
                    &file::PathAccess::with_scope(ctx.working_dir, ctx.path_scope),
                    ctx,
                )
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

/// 生成 `fn runner_git_try -> git::dispatch(args, max_len, cwd) -> Result`。`git` 是 action
/// 选择器，新增只读子命令时改 `GitAction` + `git::read_ops::dispatch`，这里不必增行。
macro_rules! define_git_runner_try {
    ($runner:ident, $git_fn:ident) => {
        #[allow(clippy::result_large_err)]
        pub fn $runner(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
            git::$git_fn(args, ctx.command_max_output_len, ctx.working_dir)
        }
    };
}

macro_rules! define_git_runners_try {
    ($( $runner:ident => $git_fn:ident ),* $(,)? ) => {
        $( define_git_runner_try!($runner, $git_fn); )*
    };
}

/// 生成 `fn runner_gh_*_try -> github_cli::gh_*_try(args, &GhCtx)`；新增 GitHub 工具时在列表中增一行。
macro_rules! gh_runner_try {
    ($name:ident, $fn:path) => {
        #[allow(clippy::result_large_err)]
        pub fn $name(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
            $fn(args, &gh_ctx_of(ctx))
        }
    };
}

/// 编译期 smoke：用真实的 `_try` 实现展开 Typed 孪生宏，防止签名漂移后宏静默失效。
///
/// args-only 形态尚无返回 `Result` 的实现，对应宏留到该族迁移 PR 再引入
/// （`git` 族已于 `git` 迁 `Typed` 时挂上）。
#[cfg(test)]
#[allow(dead_code)]
mod try_macro_typecheck {
    use super::*;

    define_runners_paths_ctx_try! {
        runner_read_file_try_smoke => crate::cm_tools::tools::file::read_file_try,
    }

    define_runners_cwd_try! {
        runner_search_in_files_try_smoke => crate::cm_tools::tools::grep_try::search_in_files_try,
    }

    define_runners_paths_try! {
        runner_glob_files_try_smoke => crate::cm_tools::tools::file::glob_files_try,
        runner_list_tree_try_smoke => crate::cm_tools::tools::file::list_tree_try,
        runner_read_dir_try_smoke => crate::cm_tools::tools::file::read_dir_try,
        runner_read_binary_meta_try_smoke => crate::cm_tools::tools::file::read_binary_meta_try,
    }

    define_git_runners_try! {
        runner_git_try_smoke => dispatch,
    }
}

// ── 需要定制逻辑的 runner（不适用同构宏）─────────────────────

/// `workflow_execute` 的同步面占位。
///
/// 真实执行由 runtime 在 `run_agent_turn` 中拦截（见 `tool_dispatch` 的
/// `Workflow` 入口）。走到这里说明拦截没生效 —— 那是内部缺陷，必须**明确失败**：
/// 从前这里返回「由运行时引擎执行（若你看到这条，说明拦截未生效）。」，正文既不含
/// 「错误」也不含「失败」，被 `looks_like_failure` 判成 `ok: true`，
/// 于是「排队一条提示后什么都没做」被报成了成功。
#[allow(clippy::result_large_err)]
pub fn runner_workflow_execute_try(
    _args: &str,
    _ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    Err(ToolError::internal_code(
        "workflow_execute_not_intercepted",
        "workflow_execute 由运行时引擎在对话回合中执行；同步工具路径未拦截（内部缺陷，请报告）。"
            .to_string(),
    ))
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
    runner_code_stats_try => code_metrics::code_stats_try,
    runner_dependency_graph_try => code_metrics::dependency_graph_try,
    runner_coverage_report_try => code_metrics::coverage_report_try,
}

/// `cargo_test` 的 Typed runner：额外透传 `ctx`（测试结果缓存 / 超时）。
#[allow(clippy::result_large_err)]
pub fn runner_cargo_test_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    cargo_tools::cargo_test_try(args, ctx.working_dir, ctx.command_max_output_len, Some(ctx))
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

/// `self_config_info` 的 Typed runner：缺 `AgentConfig` 与参数解析失败均为显式错误。
///
/// 「`sections` 未匹配任何小节」归 `invalid_args`（属调用方给了不存在的节名），
/// 正文保持原文案（含可用小节列表），便于模型自我纠正。
#[allow(clippy::result_large_err)]
pub fn runner_self_config_info_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let Some(cfg) = ctx.cfg else {
        return Err(ToolError::workspace(
            "tool_context_incomplete",
            "错误：工具上下文缺少 AgentConfig".to_string(),
        ));
    };
    let parsed: super::tool_param_types::SelfConfigInfoArgs =
        super::parse_args_typed(args).map_err(ToolError::invalid_args)?;
    let out = self_config_info::self_config_info(cfg, parsed.sections.as_deref());
    if out.is_empty() {
        return Err(ToolError::invalid_args(format!(
            "未匹配到任何配置小节；可用小节：{}。",
            self_config_info::SECTION_NAMES.join("、")
        )));
    }
    Ok(out)
}

#[allow(clippy::result_large_err)]
pub fn runner_skill_manage_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    let Some(cfg) = ctx.cfg else {
        return Err(ToolError::workspace(
            "tool_context_incomplete",
            "错误：工具上下文缺少 AgentConfig".to_string(),
        ));
    };
    let parsed: super::tool_param_types::SkillManageArgs =
        super::parse_args_typed(args).map_err(ToolError::invalid_args)?;
    skill_manage::skill_manage(parsed, &cfg.skills, ctx.working_dir)
}

#[allow(clippy::result_large_err)]
pub fn runner_apply_patch_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    patch::run_with_changelist_try(args, ctx)
}

pub fn runner_codebase_semantic_search(args: &str, ctx: &ToolContext<'_>) -> String {
    let Some(host) = ctx.codebase_semantic_host else {
        return "错误：当前执行环境未注入代码语义检索配置，无法使用 codebase_semantic_search（如部分工作流节点路径）"
            .to_string();
    };
    host.run_search(args, ctx.working_dir, ctx.command_max_output_len)
}

/// `long_term_*` / `summarize_experience` 共用入口。
///
/// 缺 `cfg` / 缺长期记忆运行时统一归 `tool_context_incomplete`（执行环境未注入，
/// 非调用方参数问题）；其余错误由 host 显式返回，不再从正文推断。
#[allow(clippy::result_large_err)]
fn dispatch_long_term_tool(
    name: &str,
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let Some(cfg) = ctx.cfg else {
        return Err(ToolError::workspace(
            "tool_context_incomplete",
            "错误：工具上下文缺少 AgentConfig".to_string(),
        ));
    };
    let Some(host) = ctx.long_term_memory_host else {
        return Err(ToolError::workspace(
            "tool_context_incomplete",
            "错误：当前会话未挂载长期记忆运行时（或未启用持久化）".to_string(),
        ));
    };
    host.dispatch(name, args, cfg)
}

#[allow(clippy::result_large_err)]
pub fn runner_long_term_remember_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    dispatch_long_term_tool("long_term_remember", args, ctx)
}

#[allow(clippy::result_large_err)]
pub fn runner_summarize_experience_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    dispatch_long_term_tool("summarize_experience", args, ctx)
}

#[allow(clippy::result_large_err)]
pub fn runner_long_term_forget_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    dispatch_long_term_tool("long_term_forget", args, ctx)
}

#[allow(clippy::result_large_err)]
pub fn runner_long_term_memory_list_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    dispatch_long_term_tool("long_term_memory_list", args, ctx)
}

#[allow(clippy::result_large_err)]
pub fn runner_background_job_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    background_job_tools::background_job_try(
        args,
        ctx.tool_jobs_host,
        ctx.working_dir,
        ctx.command_max_output_len,
    )
}

#[allow(clippy::result_large_err)]
pub fn runner_background_job_cancel_try(
    args: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    background_job_tools::background_job_cancel_try(args, ctx.tool_jobs_host)
}

#[allow(clippy::result_large_err)]
pub fn read_file_try_dispatch(
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::cm_tools::tool_result::ToolError> {
    file::read_file_try(args_json, &file::PathAccess::new(ctx.working_dir), ctx)
}

/// 用户消息 `@路径` 展开等：与 `read_file` 工具同源校验与读取。
#[allow(clippy::result_large_err)]
pub fn read_file_try_at_paths(
    args_json: &str,
    working_dir: &std::path::Path,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::cm_tools::tool_result::ToolError> {
    file::read_file_try(args_json, &file::PathAccess::new(working_dir), ctx)
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

pub fn runner_crate_contract_map(args: &str, ctx: &ToolContext<'_>) -> String {
    contract_map::crate_contract_map(args, ctx)
}

// ── 同构薄封装实例（形态 1：f(args, cwd, max_len)）────────────

define_runners_cwd_maxlen! {
    runner_repo_overview_sweep => repo_overview::repo_overview_sweep,
    runner_docs_health_sweep => docs_health_sweep::docs_health_sweep,
}

// ── 同构薄封装实例（形态 2 孪生：f(args, cwd, ctx) -> Result）──

define_runners_cwd_ctx_try! {
    runner_structured_try => structured_dispatch::structured_try,
}

// ── 同构薄封装实例（形态 paths_ctx：f(args, &PathAccess, ctx) -> Result）──

// `archive-tools` 未启用时 `archive` 模块不参与编译（见 `tools/mod.rs` 的 cfg）。
// 归档的读（源目录 / 待解压文件）与写（输出归档 / 解压目录）都要过 `PathAccess`：
// 越界与绝对路径在校验层挡住，故走 `paths_ctx` 形态而非 `cwd_ctx`。
#[cfg(feature = "archive-tools")]
define_runners_paths_ctx_try! {
    runner_archive_pack_try => archive::archive_pack,
    runner_archive_unpack_try => archive::archive_unpack,
    runner_archive_list_try => archive::archive_list,
}

define_runners_paths_ctx_try! {
    runner_create_file_try => file::create_file_try,
    runner_copy_file_try => file::copy_file_try,
    runner_move_file_try => file::move_file_try,
    runner_modify_file_try => file::modify_file_try,
    runner_delete_files_try => file::delete_files_try,
    runner_append_file_try => file::append_file_try,
    runner_search_replace_try => file::search_replace_try,
}

// ── 同构薄封装实例（形态 3 孪生：f(args, cwd) -> Result）──────

define_runners_cwd_try! {
    runner_text_diff_try => text_diff::run_try,
    runner_table_text_try => table_text::run_try,
    runner_markdown_check_links_try => markdown_links::markdown_check_links_try,
    runner_reminder_try => schedule_dispatch::reminder,
    runner_event_try => schedule_dispatch::event,
}

// ── 已改走 `PathAccess` 的文件类工具（R1 收口）──────────────────
// S3 只需改 `define_runners_paths_try!` 的构造点，即可给全部文件工具注入路径范围。

define_runners_paths_try! {
    // file_core 只读族 B4：显式 ToolError（read_dir 同样覆盖工作区外路径的审批前置场景）。
    runner_delete_dir_try => file::delete_dir_try,
    runner_create_dir_try => file::create_dir_try,
    runner_read_dir_try => file::read_dir_try,
    runner_glob_files_try => file::glob_files_try,
    runner_list_tree_try => file::list_tree_try,
    runner_read_binary_meta_try => file::read_binary_meta_try,
}

// ── 同构薄封装实例（形态 3：f(args, cwd)）────────────────────

define_runners_cwd! {
    runner_code_nav => code_nav_dispatch::dispatch,
}

/// `rust_analyzer`：8 个 action 的 LSP 请求，失败一律显式 [`ToolError`]。
///
/// 成功正文保持原样（golden 与下游子串匹配依赖它），只把失败路径从「文本猜」换成
/// `Err`。见 `rust_ide::rust_analyzer_try`。
#[allow(clippy::result_large_err)]
pub fn runner_rust_analyzer_try(args: &str, ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    rust_ide::rust_analyzer_try(args, ctx.working_dir)
}

// ── 同构薄封装实例（形态 4：f(args)）─────────────────────────

define_runners_args_only! {
    runner_present_clarification_questionnaire => crate::cm_tools::clarification_questionnaire::run_present_clarification_questionnaire,
}

// ── misc_basic 族（args-only 形态；仅 4 个条目，手写具名 runner，不抽宏）──

/// `regex_test`：缺参/超限为 `invalid_args`，正则编译失败为 `regex_test_invalid_regex`。
#[allow(clippy::result_large_err)]
pub fn runner_regex_test_try(args: &str, _ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    regex_test::run_try(args)
}

/// `date_calc`：diff/offset 的缺参与日期解析失败均为 `invalid_args`。
#[allow(clippy::result_large_err)]
pub fn runner_date_calc_try(args: &str, _ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    date_calc::run_try(args)
}

/// `json_format`：解析失败为 `invalid_args`；序列化失败为内部错误。
#[allow(clippy::result_large_err)]
pub fn runner_json_format_try(args: &str, _ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    json_format::run_try(args)
}

/// `env_var_check`：参数缺失 / 为空 / 超上限均为 `invalid_args`。
#[allow(clippy::result_large_err)]
pub fn runner_env_var_check_try(args: &str, _ctx: &ToolContext<'_>) -> Result<String, ToolError> {
    env_var_check::run_try(args)
}

// ── Git 工具 ────────────────────────────────────────────────

// `git` 已迁 `Typed`（见 `docs/design/tool_runner_typed_migration.md` §4.12）：
// 子命令非零退出、白名单外目录、起不来都有各自的 `error_code`，不再靠正文关键词。
define_git_runners_try! {
    runner_git_try => dispatch,
}

// ── GitHub CLI 工具 ──────────────────────────────────────

// `gh_*` 已全部迁 `Typed`（见 `docs/design/tool_runner_typed_migration.md` §4.3）：
// `gh` 非零退出、白名单拒绝、起不来都有各自的 `error_code`，不再靠正文关键词。
gh_runner_try!(runner_gh_pr_try, github_cli::gh_pr_try);
gh_runner_try!(runner_gh_run_try, github_cli::gh_run_try);
gh_runner_try!(
    runner_gh_issue_create_try,
    github_cli::gh_issue_create_try
);
gh_runner_try!(
    runner_gh_release_create_try,
    github_cli::gh_release_create_try
);
gh_runner_try!(runner_gh_api_try, github_cli::gh_api_try);

/// `ToolContext` 的三件套投影（`gh_runner!` 与上面两个分发器共用）。
fn gh_ctx_of<'a>(ctx: &'a ToolContext<'a>) -> github_cli::GhCtx<'a> {
    github_cli::GhCtx::new(
        ctx.command_max_output_len,
        ctx.allowed_commands,
        ctx.working_dir,
    )
}
