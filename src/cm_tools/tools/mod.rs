//! 工具定义与执行：文件、命令、网络与开发工具
//!
//! 每个子模块对应一类工具，便于扩展新工具。

#[cfg(feature = "archive-tools")]
mod archive;
mod background_job_tools;
mod call_graph_sketch;
pub(crate) mod cargo_tools;
mod code_metrics;
mod code_nav;
mod command;
mod command_error;
mod command_exec;
mod command_outcome;
pub use command_outcome::{CommandDigest, CommandOutcome, ExitStatusKind};
pub use command::{
    CommandSpec, PreparedRunCommand, RunCommandTestCacheOpts, RunOptions, allowlist_allows_all,
    argv_has_shell_operators, argv_needs_posix_shell_wrap, argv_needs_shell_expansion,
    dash_c_script_body, is_shell_dash_c_invocation,
    join_run_command_argv_tail, join_run_command_shell_script, parse_command_spec,
    peel_cd_prefix_argv_for_shell_policy, posix_shell_on_allowlist,
    prepare_run_command_for_pty_spawn, prepare_run_command_for_pty_spawn_spec, run, run_async,
    scan_run_command_unsafe_args_spec,
};
pub use command_error::RunCommandError;
pub(crate) use command::{parse_run_command_payload_lenient, parse_run_command_value_with_repair};
mod command_line_prepare;
pub use command_line_prepare::split_command_prefix_if_embedded;
mod contract_map;
mod date_calc;
mod debug_tools;
mod diagnostics;
mod docs_health_sweep;
pub use diagnostics::capture_trimmed;
pub use parse_args::parse_args_from_value;
pub use parse_args::parse_args_json;
pub use parse_args::parse_args_typed;
mod env_var_check;
mod error_playbook;
mod exec;
pub use exec::{
    resolve_workspace_executable, run_command_invocation_targets_workspace_script_or_executable,
};
mod file;
pub use file::canonical_workspace_root;
pub use file::list_tree;
pub(crate) use file::resolve_for_read_open;
mod frontend_tools;
mod git;
mod github_cli;
mod grep_try;
#[cfg(feature = "web-fetch-tools")]
pub mod http_fetch;
mod json_format;
mod lint;
mod markdown_links;
pub mod output_util;
mod parse_args;
mod patch;
pub(crate) mod python_tools;
mod regex_test;
mod release_docs;
mod repo_overview;
mod rust_ide;
mod schedule;
mod schedule_dispatch;
mod self_config_info;
mod skill_manage;
mod spell_tools;
#[cfg(feature = "csv-tools")]
mod structured_data;
pub mod structured_preview;
mod symbol;
#[cfg(feature = "csv-tools")]
mod table_text;
mod test_result_cache;
#[cfg(feature = "diff-tools")]
mod text_diff;
mod text_transform;
#[cfg(feature = "schema-tools")]
mod tool_args_validate;
mod tool_json_schema;
mod tool_param_types;
mod tool_specs_registry;
mod tool_summary;
mod tool_summary_args;
mod weather;
mod web_search;
mod workspace_image_chat_hint;
mod write_sse_preview;

pub mod dev_tag;

use std::sync::Arc;

use crate::cm_tools::memory_tool_host::{
    CodebaseSemanticToolHost, LongTermMemoryToolHost, ToolJobsToolHost,
};
use crate::cm_tools::tool_result::{ToolError, ToolResult};
use crate::cm_tools::workspace::changelist::WorkspaceChangelist;
use crate::cm_config::{AgentConfig, ExposeSecret};
use crate::cm_types::{FunctionDef, Tool};

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：天气、联网搜索与受控 HTTP、文本/格式转换、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

impl ToolCategory {
    /// 对外稳定枚举字面量：`basic` / `development`（供 `GET /tools` 投影）。
    pub fn as_str(self) -> &'static str {
        match self {
            ToolCategory::Basic => "basic",
            ToolCategory::Development => "development",
        }
    }
}

pub struct ToolContext<'a> {
    /// 主 Agent / `tool_context_for*` 路径填充；工作流节点等自建上下文可为 `None`（部分工具将报错）。
    pub cfg: Option<&'a AgentConfig>,
    /// 代码语义检索；主 Agent 路径由 `cm_internal` 注入。
    pub codebase_semantic_host: Option<&'a dyn CodebaseSemanticToolHost>,
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    pub allowed_commands: &'a [String],
    pub working_dir: &'a std::path::Path,
    pub web_search_timeout_secs: u64,
    pub web_search_provider: crate::cm_config::WebSearchProvider,
    pub web_search_api_key: &'a str,
    pub web_search_max_results: u32,
    /// worbrow 浏览器/静态抓取代理（`http://host:port` / `https://host:port`；空 = 直连/系统代理）。
    /// 仅 `web_search_provider = worbrow` 时透传给 worbrow `Config::with_proxy`。
    pub web_search_proxy: &'a str,
    pub http_fetch_allowed_prefixes: &'a [String],
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    /// `run_command` / 部分子进程工具的墙上时钟上限（秒）；`python_snippet_run` 等复用。
    pub command_timeout_secs: u64,
    /// 单轮 `run_agent_turn` 内 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<&'a crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>,
    /// 本会话工作区变更集（按 `long_term_memory_scope_id`）；`None` 时不记录。
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// `cargo_test` / 部分 `run_command cargo test` 的进程内输出缓存。
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
    /// 长期记忆工具；主 Agent 路径由 `cm_internal` 注入。
    pub long_term_memory_host: Option<&'a dyn LongTermMemoryToolHost>,
    /// 后台任务查询工具（`background_job_status` / `background_job_list`）；主 Agent 路径由 `cm_internal` 注入。
    pub tool_jobs_host: Option<&'a dyn ToolJobsToolHost>,
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
/// 与内置文件工具相同的路径规则：将相对路径解析为工作区内绝对路径（供变更集等跨模块只读）。
pub use crate::cm_tools::workspace::path::WorkspacePathError;

pub fn resolve_workspace_path_for_read(
    working_dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, WorkspacePathError> {
    file::resolve_for_read(working_dir, rel)
}

pub fn tool_context_for<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
) -> ToolContext<'a> {
    ToolContext {
        cfg: Some(cfg),
        codebase_semantic_host: None,
        command_max_output_len: cfg.command_exec.command_max_output_len,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider,
        web_search_api_key: cfg.web_search.web_search_api_key.expose_secret(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        web_search_proxy: cfg.web_search.web_search_proxy.as_str(),
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        long_term_memory_host: None,
        tool_jobs_host: None,
    }
}

/// 在 [`tool_context_for`] 基础上挂载单轮 `read_file` 缓存、会话变更集与可选记忆/后台任务宿主。
#[allow(clippy::too_many_arguments)] // 逐项注入可选宿主（语义索引 / 长期记忆 / 后台任务），展开为结构体反而更绕
pub fn tool_context_for_with_read_cache_and_memory<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<&'a crate::cm_tools::read_file_turn_cache::ReadFileTurnCache>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    codebase_semantic_host: Option<&'a dyn CodebaseSemanticToolHost>,
    long_term_memory_host: Option<&'a dyn LongTermMemoryToolHost>,
    tool_jobs_host: Option<&'a dyn ToolJobsToolHost>,
) -> ToolContext<'a> {
    ToolContext {
        read_file_turn_cache,
        workspace_changelist,
        codebase_semantic_host,
        long_term_memory_host,
        tool_jobs_host,
        ..tool_context_for(cfg, allowed_commands, working_dir)
    }
}

mod runners;
pub use runners::*;

/// 已装配命令的纯数据快照（`std::process::Command` 非 `Send`，无法跨线程复用）。
///
/// 后台任务（async）按同一份 `program + args + cwd` 复刻命令，保证后台与前台
/// 的 argv / 工作目录不漂移。
pub(crate) struct AssembledCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    /// 装配时设定的工作目录（`Command::current_dir`）；`None` = 未显式设定。
    pub(crate) cwd: Option<std::path::PathBuf>,
}

/// 从已装配的 [`std::process::Command`] 提取可复刻快照。
pub(crate) fn command_program_and_args(cmd: &std::process::Command) -> AssembledCommand {
    AssembledCommand {
        program: cmd.get_program().to_string_lossy().into_owned(),
        args: cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        cwd: cmd.get_current_dir().map(std::path::Path::to_path_buf),
    }
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
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(spec.name) {
        return false;
    }
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
pub fn cached_params_for_tool_name(name: &str) -> Option<serde_json::Value> {
    find_spec(name).map(cached_params)
}

/// 编译期注册表项的只读投影（`GET /tools` 内置源）。
///
/// 暴露**全部**注册项（含 `fastembed` 特性门剔除者），是否「在场」由调用方按
/// [`tool_passes_filters`] 语义自行判定，便于报告 `absent_reason`。
pub struct BuiltinToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ToolCategory,
    /// `Development` 类工具的子域标签；`Basic` 类为空。
    pub dev_tags: &'static [&'static str],
    /// 是否需要 `fastembed` Cargo feature（当前构建未启用则不在场）。
    pub requires_fastembed: bool,
}

/// 枚举内置工具注册表（进程级 `&'static` 表，一次构建）。
pub fn builtin_tool_descriptors() -> Vec<BuiltinToolDescriptor> {
    tool_specs()
        .iter()
        .map(|s| BuiltinToolDescriptor {
            name: s.name,
            description: s.description,
            category: s.category,
            dev_tags: match s.category {
                ToolCategory::Development => dev_tag::tags_for_tool_name(s.name),
                ToolCategory::Basic => &[],
            },
            requires_fastembed: tool_spec_requires_fastembed(s.name),
        })
        .collect()
}

/// 执行本地工具并返回结果字符串。
/// `ToolContext` 聚合 `run_command`、`get_weather`、`web_search` 等工具所需的配置项。
pub fn run_tool(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(name) {
        return "错误：codebase_semantic_search 需要 `fastembed` Cargo feature；当前构建未启用。"
            .to_string();
    }
    match find_spec(name) {
        Some(spec) => {
            let args_eff =
                match tool_args_validate::effective_builtin_tool_args_json(name, args_json) {
                    Ok(c) => c,
                    Err(e) => return format!("错误：{e}"),
                };
            if let Some(Err(e)) =
                tool_args_validate::validate_parsed_str_for_builtin(name, args_eff.as_ref())
            {
                return format!("错误：{e}");
            }
            match spec.runner {
                ToolRunner::Legacy(f) => f(args_eff.as_ref(), ctx),
                // Typed 的成败由 `Result` 决定；`run_tool` 只返回 String，故在不一致时注入
                // 显式 `ok` 头，供下游 `parse_legacy_output` 复用（见 `typed_outcome_output`）。
                ToolRunner::Typed(f) => match f(args_eff.as_ref(), ctx) {
                    Ok(output) => typed_outcome_output(name, true, None, output),
                    Err(e) => typed_outcome_output(name, false, Some(e.code.as_str()), e.message),
                },
                ToolRunner::AsyncOnly => async_only_sync_error(name),
            }
        }
        None => format!("未知工具：{}", name),
    }
}

/// 按 `spec.runner` 执行：已迁移的 [`ToolRunner::Typed`] 显式透传 [`ToolError`]，
/// [`ToolRunner::Legacy`] 经 `parse_legacy_output` 从正文推断；
/// [`ToolRunner::AsyncOnly`] 在同步面无执行体，返回提示性 [`ToolError`]。
///
/// **`run_command` 不在本函数执行**：它经专用异步入口 [`run_tool_async`] / [`run_tool_result_async`]
/// 走 [`run_command_dispatch_async`]。
#[allow(clippy::result_large_err)]
fn run_tool_dispatch(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tools::tool_result::ParsedLegacyOutput), ToolError> {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(name) {
        return Err(ToolError::invalid_args(
            "codebase_semantic_search 需要 `fastembed` Cargo feature；当前构建未启用。".to_string(),
        ));
    }
    let spec = match find_spec(name) {
        Some(s) => s,
        None => return Err(ToolError::unknown_tool(name)),
    };
    let args_eff = tool_args_validate::effective_builtin_tool_args_json(name, args_json)
        .map_err(ToolError::invalid_args)?;
    let args_ref = args_eff.as_ref();
    if let Some(Err(e)) = tool_args_validate::validate_parsed_str_for_builtin(name, args_ref) {
        return Err(ToolError::invalid_args(e));
    }
    run_tool_dispatch_fallback(name, args_ref, spec, ctx)
}

/// dispatch 兜底：按 `spec.runner` 执行。
///
/// Typed：`Ok` 即成功（正文只用于抽取流/退出码），`Err(ToolError)` 显式透传；
/// Legacy：正文推断失败状态（与历史 `run_tool` fallback 行为一致）；
/// AsyncOnly：同步面不可执行。
#[allow(clippy::result_large_err)]
fn run_tool_dispatch_fallback(
    name: &str,
    args_ref: &str,
    spec: &ToolSpec,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tools::tool_result::ParsedLegacyOutput), ToolError> {
    match spec.runner {
        ToolRunner::Typed(f) => {
            f(args_ref, ctx).map(|output| finish_dispatch_typed_ok(name, args_ref, output))
        }
        ToolRunner::Legacy(f) => finish_dispatch_legacy(name, args_ref, f(args_ref, ctx)),
        ToolRunner::AsyncOnly => Err(ToolError::invalid_args(async_only_message(name))),
    }
}

/// 同步面无执行体时的统一说明（[`run_tool`] 前置「错误：」，[`run_tool_try`] 包成 [`ToolError`]）。
fn async_only_message(name: &str) -> String {
    format!("{name} 须由服务端异步调度执行。")
}

fn async_only_sync_error(name: &str) -> String {
    format!("错误：{}", async_only_message(name))
}

/// Typed 结果 → String 面：仅在正文启发式与真实成败/错误码不一致时注入显式头。
///
/// 主 Agent 路径（[`run_tool`]）丢失了 [`ToolError`]，下游 `dispatch_tool` 重试判定、
/// timeline / 模型信封、统计与验收又都用 `parse_legacy_output` 重新推断。正文一致时
/// 保持逐字节不变；不一致时用 `crabmate_tool_output` 头声明真实成败与 `error_code`。
fn typed_outcome_output(
    name: &str,
    ok: bool,
    error_code: Option<&str>,
    output: String,
) -> String {
    let parsed = crate::cm_tools::tool_result::parse_legacy_output(name, &output);
    if parsed.ok == ok
        && (ok || error_code.is_none() || parsed.error_code.as_deref() == error_code)
    {
        return output;
    }
    crate::cm_tools::tool_result::prepend_crabmate_tool_output_with_ok_and_error_code(
        name,
        ok,
        error_code,
        serde_json::json!({}),
        &output,
    )
}

/// Typed 收尾：`Ok` 即成功，正文只用于抽取 `exit_code/stdout/stderr`。
///
/// 成功正文（如搜索结果）可能含「失败」「超时」等词，因此**不能**再把正文交给
/// [`crate::cm_tools::tool_result::parse_legacy_output`] 推断 `ok`；失败必须由执行体
/// 返回 [`ToolError`]。返回值同样注入显式 `ok` 头（若必要），保证下游再次解析一致。
fn finish_dispatch_typed_ok(
    name: &str,
    args_json: &str,
    output: String,
) -> (String, crate::cm_tools::tool_result::ParsedLegacyOutput) {
    let mut parsed = crate::cm_tools::tool_result::parse_legacy_output(name, &output);
    parsed.ok = true;
    parsed.error_code = None;
    if parsed.exit_code.is_some_and(|code| code != 0) {
        parsed.exit_code = None;
    }
    let output = typed_outcome_output(name, true, None, output);
    let output = workspace_image_chat_hint::append_if_needed(args_json, output);
    (output, parsed)
}

/// Legacy 收尾：先按原始正文推断成功/失败（**早于**图片提示附加），`ok` 时附加工作区图片提示，否则转 [`ToolError`]。
#[allow(clippy::result_large_err)]
fn finish_dispatch_legacy(
    name: &str,
    args_json: &str,
    output: String,
) -> Result<(String, crate::cm_tools::tool_result::ParsedLegacyOutput), ToolError> {
    let parsed = crate::cm_tools::tool_result::parse_legacy_output(name, &output);
    if parsed.ok {
        let output = workspace_image_chat_hint::append_if_needed(args_json, output);
        Ok((output, parsed))
    } else {
        Err(ToolError::from_parsed_legacy(name, &parsed, output))
    }
}

/// 与 [`run_tool`] 相同，但失败时返回 [`crate::cm_tools::tool_result::ToolError`]（含 **分类 / 错误码 / retryable**）。
///
/// `run_command` / `terminal_session` 的 spec runner 为 [`ToolRunner::AsyncOnly`]（须经服务端异步调度）；
/// 其余工具按 `spec.runner` 执行：已迁移的 [`ToolRunner::Typed`] 显式透传，[`ToolRunner::Legacy`] 由 [`crate::cm_tools::tool_result::parse_legacy_output`] 从正文推断。
#[allow(dead_code, clippy::result_large_err)] // 供编排与单测显式 `Result` 分支；主路径现经 [`run_tool_dispatch`] + [`run_tool_result`]
pub fn run_tool_try(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, crate::cm_tools::tool_result::ToolError> {
    run_tool_dispatch(name, args_json, ctx).map(|(output, _)| output)
}

/// 执行本地工具并返回结构化结果（兼容既有字符串输出）。
pub fn run_tool_result(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> ToolResult {
    match run_tool_dispatch(name, args_json, ctx) {
        Ok((output, parsed)) => ToolResult::from_parsed(output, parsed),
        Err(e) => ToolResult::from_parsed(e.message, e.legacy_parsed),
    }
}

/// 异步工具执行入口（面向非 chat 消费点：tool-replay / MCP / workflow / 容器 runner）。
///
/// **`run_command`** 经本文件新增的异步实现 [`run_command_dispatch_async`]（[`run_tool_async`] 的 String 面）；
/// 其余工具直接委派既有同步 [`run_tool`]，行为逐字节保持不变。
///
/// `run_command` 实现体基于 [`tokio::process`] 事件驱动等待（含 `ctx.command_timeout_secs` 墙钟超时与测试缓存），
/// 不再占用阻塞线程。
pub async fn run_tool_async(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    if name == "run_command" {
        match run_command_dispatch_async(name, args_json, ctx).await {
            Ok((output, _)) => output,
            Err(e) => e.message,
        }
    } else {
        run_tool(name, args_json, ctx)
    }
}

/// 异步版 [`run_tool_result`]：`run_command` 走 [`run_command_dispatch_async`]，其余委派同步路径。
pub async fn run_tool_result_async(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> ToolResult {
    if name == "run_command" {
        match run_command_dispatch_async(name, args_json, ctx).await {
            Ok((output, parsed)) => ToolResult::from_parsed(output, parsed),
            Err(e) => ToolResult::from_parsed(e.message, e.legacy_parsed),
        }
    } else {
        run_tool_result(name, args_json, ctx)
    }
}

/// 新的异步 `run_command` 实现（解析一次 `CommandSpec`，再以 [`command::run_async`] 异步等待）。
///
/// 实现体：解析一次 [`command::CommandSpec`] 后，以 [`tokio::process`] 异步等待
/// [`command::run_async`]（不占用阻塞线程）；
/// 借用字段（`allowed_commands` / `working_dir`）先 owned 化以延长生命周期；github token 经
/// [`with_request_github_token`](crate::cm_tools::github_token::with_request_github_token) 以 async 作用域注入
/// （命令构建与等待同一 async 任务，task-local 自然可用）；成功/失败收尾复用 [`finish_dispatch_legacy`]。
#[allow(clippy::result_large_err)]
async fn run_command_dispatch_async(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::cm_tools::tool_result::ParsedLegacyOutput), ToolError> {
    let name_owned = name.to_string();
    let spec = command::parse_command_spec(args_json, ctx.working_dir)
        .map_err(RunCommandError::into_tool_error)?;
    let max_out = ctx.command_max_output_len;
    let allowed: Vec<String> = ctx.allowed_commands.to_vec();
    let work_dir = ctx.working_dir.to_path_buf();
    let test_cache_enabled = ctx.test_result_cache_enabled;
    let test_cache_max = ctx.test_result_cache_max_entries;
    let timeout_secs = ctx.command_timeout_secs;
    let github_token = crate::cm_tools::github_token::resolve_token_plaintext();
    let fut = async move {
        let test_cache = test_cache_enabled.then_some(command::RunCommandTestCacheOpts {
            enabled: true,
            max_entries: test_cache_max,
            workspace_root: work_dir.as_path(),
        });
        let wait =
            crate::cm_tools::subprocess_session::SubprocessWaitCtl::with_wall_secs(timeout_secs);
        command::run_async(
            &spec,
            command::RunOptions {
                max_output_len: max_out,
                allowed_commands: &allowed,
                working_dir: work_dir.as_path(),
                test_cache,
                skip_arg_safety: false,
                wait: &wait,
            },
        )
        .await
        .map_err(RunCommandError::into_tool_error)
        .and_then(|outcome| {
            let rendered = outcome.render(max_out);
            match outcome.session_stop_tool_error(&rendered) {
                Some(e) => Err(e),
                None => finish_dispatch_legacy(&name_owned, &spec.args_json, rendered),
            }
        })
    };
    crate::cm_tools::github_token::with_request_github_token(github_token, fut).await
}

/// 判断本次 run_command 是否为“成功的编译命令”（常见 C/C++ 构建工具且退出码为 0）
pub fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let Ok(spec) = parse_command_spec(args_json, std::path::Path::new(".")) else {
        return false;
    };
    is_compile_command_success_spec(&spec, result)
}

/// [`is_compile_command_success`] 的已解析版本。
#[must_use]
pub fn is_compile_command_success_spec(spec: &CommandSpec, result: &str) -> bool {
    if !is_compile_command_spec(spec) {
        return false;
    }
    let parsed = crate::cm_tools::tool_result::parse_legacy_output("run_command", result);
    parsed.ok && parsed.exit_code == Some(0)
}

/// 仅判断命令形态是否为编译命令（供后台任务等没有结果正文的消费方）。
#[must_use]
pub fn is_compile_command_spec(spec: &CommandSpec) -> bool {
    if let Some(script) = spec.script.as_deref() {
        return script_contains_compile_command(script);
    }
    let mut cmd = spec.command.trim().to_string();
    let mut args = spec.args.clone();
    // 兼容 `command` 为空、编译工具落在 `args` 首项的旧形态。
    if cmd.is_empty() {
        if args.is_empty() {
            return false;
        }
        cmd = args.remove(0);
    }
    let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(&cmd, &args);
    if is_shell_dash_c_invocation(&cmd, &args) {
        return dash_c_script_body(&args).is_some_and(script_contains_compile_command);
    }
    command_invocation_is_compile(&cmd, &args)
}

fn script_contains_compile_command(script: &str) -> bool {
    for part in script.split(['&', '|', ';', '\n', '\r']) {
        let words = crate::cmd_mate::split_command_line(part);
        let Some((cmd, rest)) = words.split_first() else {
            continue;
        };
        if cmd.trim().is_empty() {
            continue;
        }
        let (cmd, args) = peel_cd_prefix_argv_for_shell_policy(cmd, rest);
        if is_shell_dash_c_invocation(&cmd, &args) {
            if dash_c_script_body(&args).is_some_and(script_contains_compile_command) {
                return true;
            }
            continue;
        }
        if command_invocation_is_compile(&cmd, &args) {
            return true;
        }
    }
    false
}

fn command_invocation_is_compile(cmd: &str, args: &[String]) -> bool {
    let base = cmd
        .rsplit('/')
        .next()
        .unwrap_or(cmd)
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "gcc" | "g++" | "cc" | "c++" | "clang" | "clang++" | "make" | "cmake" | "ninja"
        | "rustc" => true,
        "cargo" => args
            .first()
            .is_some_and(|a| matches!(a.as_str(), "build" | "check" | "rustc")),
        _ => false,
    }
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
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
pub fn summarize_tool_call_parsed(name: &str, args_parsed: &serde_json::Value) -> Option<String> {
    let spec = find_spec(name)?;
    match &spec.summary {
        ToolSummaryKind::None => None,
        ToolSummaryKind::Static(s) => Some((*s).to_string()),
        ToolSummaryKind::Dynamic(f) => f(args_parsed),
    }
}

/// 仅 **`cargo test`**：重置少数进程级可变 `static`（`run_command` 秒级限流、**`test_result_cache`** LRU）。
/// 集成测试可在夹具开头调用 crate 根 **[`crate::cm_tools::reset_process_tool_globals_for_tests`]**。
pub fn reset_process_tool_globals_for_tests() {
    command::reset_run_command_rate_limit_for_tests();
    test_result_cache::reset_test_result_cache_for_tests();
}

pub use github_cli::web_api;

#[cfg(test)]
#[path = "mod/tests.rs"]
mod tests;

#[cfg(test)]
mod testing;
