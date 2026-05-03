//! 工具定义与执行：时间、计算(bc)、有限 Linux 命令
//!
//! 每个子模块对应一类工具，便于扩展新工具。

mod archive;
mod calc;
mod call_graph_sketch;
mod cargo_tools;
mod ci_tools;
mod code_metrics;
mod code_nav;
mod command;
mod container_tools;
mod contract_map;
mod date_calc;
mod debug_tools;
mod diagnostics;
mod docs_health_sweep;
pub(crate) use diagnostics::capture_trimmed;
pub(crate) use parse_args::parse_args_json;
mod env_var_check;
mod error_playbook;
mod exec;
pub(crate) use exec::{
    resolve_workspace_executable, run_command_invocation_targets_workspace_script_or_executable,
};
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
mod long_term_memory_tools;
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
pub(crate) mod structured_preview;
mod symbol;
mod table_text;
mod test_result_cache;
mod text_diff;
mod text_transform;
mod time;
mod todo_scan;
mod tool_args_validate;
mod tool_json_schema;
mod tool_param_types;
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
use crate::tool_result::{ToolError, ToolResult};
use crate::types::{FunctionDef, Tool};
use crate::workspace::changelist::WorkspaceChangelist;
use crate::workspace::path::{validate_effective_workspace_base, validate_workspace_set_path};

/// 工具顶层分类（用于 `build_tools_filtered`、文档与后续按场景裁剪工具列表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// 基础工具：时间/计算/天气、联网搜索与受控 HTTP、日程与提醒等（不依赖「在仓库里写代码」）。
    Basic,
    /// 开发工具：工作区文件、Git、Cargo/前端构建与测试、Lint、补丁、符号搜索、工作流等。
    Development,
}

pub struct ToolContext<'a> {
    /// 主 Agent / `tool_context_for*` 路径填充；工作流节点等自建上下文可为 `None`（部分工具将报错）。
    pub cfg: Option<&'a AgentConfig>,
    /// 代码语义检索参数；主 Agent 路径由 `tool_context_for*` 从 [`AgentConfig`] 填充，其它路径为 `None` 时该工具不可用。
    pub codebase_semantic:
        Option<crate::memory::codebase_semantic_index::CodebaseSemanticToolParams>,
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
    /// `run_command` / 部分子进程工具的墙上时钟上限（秒）；`python_snippet_run` 等复用。
    pub command_timeout_secs: u64,
    /// 单轮 `run_agent_turn` 内 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<&'a crate::read_file_turn_cache::ReadFileTurnCache>,
    /// 本会话工作区变更集（按 `long_term_memory_scope_id`）；`None` 时不记录。
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// `cargo_test` / `npm run test` / 部分 `run_command cargo test` 的进程内输出缓存。
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
    /// 长期记忆运行时与会话作用域（供 `long_term_*` 工具）；缺省为 `None`。
    pub long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
}

/// 由 [`AgentConfig`] 与当前工作目录、命令白名单构造工具上下文（供 `run_tool` 使用）。
/// 与内置文件工具相同的路径规则：将相对路径解析为工作区内绝对路径（供变更集等跨模块只读）。
pub use crate::workspace::path::WorkspacePathError;

pub fn resolve_workspace_path_for_read(
    working_dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, WorkspacePathError> {
    file::resolve_for_read(working_dir, rel)
}

/// REPL **`/workspace`** / **`/cd`**：相对路径走 [`resolve_workspace_path_for_read`]（与 `read_file` 等一致：**禁止**以 `/` 开头的绝对路径）；绝对路径走 [`crate::workspace::path::validate_workspace_set_path`]（与 Web **`POST /workspace`** 一致：`workspace_allowed_roots` + 敏感目录黑名单）。
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
        cfg: Some(cfg),
        codebase_semantic: Some(
            crate::memory::codebase_semantic_index::CodebaseSemanticToolParams::from_agent_config(
                cfg,
            ),
        ),
        command_max_output_len: cfg.command_exec.command_max_output_len,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        allowed_commands,
        working_dir,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider,
        web_search_api_key: cfg.web_search.web_search_api_key.expose_secret(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.as_slice(),
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        long_term_memory: None,
        long_term_memory_scope_id: None,
    }
}

/// 在 [`tool_context_for`] 基础上挂载单轮 `read_file` 缓存、会话变更集与可选长期记忆（供 `dispatch_tool` / `execute_tools`）。
pub fn tool_context_for_with_read_cache_and_memory<'a>(
    cfg: &'a AgentConfig,
    allowed_commands: &'a [String],
    working_dir: &'a std::path::Path,
    read_file_turn_cache: Option<&'a crate::read_file_turn_cache::ReadFileTurnCache>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    long_term_memory: Option<
        std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>,
    >,
    long_term_memory_scope_id: Option<String>,
) -> ToolContext<'a> {
    ToolContext {
        read_file_turn_cache,
        workspace_changelist,
        long_term_memory,
        long_term_memory_scope_id,
        ..tool_context_for(cfg, allowed_commands, working_dir)
    }
}

mod runners;
pub(crate) use runners::*;

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
pub(crate) fn cached_params_for_tool_name(name: &str) -> Option<serde_json::Value> {
    find_spec(name).map(cached_params)
}

pub(crate) use schema_check::workflow_tool_args_satisfy_required;

/// 执行本地工具并返回结果字符串。
/// `ToolContext` 聚合 `run_command`、`get_weather`、`web_search` 等工具所需的配置项。
pub fn run_tool(name: &str, args_json: &str, ctx: &ToolContext<'_>) -> String {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(name) {
        return "错误：codebase_semantic_search 需要 `fastembed` Cargo feature；当前构建未启用。"
            .to_string();
    }
    match find_spec(name) {
        Some(spec) => {
            if let Some(Err(e)) =
                tool_args_validate::validate_parsed_str_for_builtin(name, args_json)
            {
                return format!("错误：{e}");
            }
            (spec.runner)(args_json, ctx)
        }
        None => format!("未知工具：{}", name),
    }
}

/// `run_command` 与 `cargo_*` / `rust_test_one` / `rust_rustc` 走显式 [`ToolError`]；其余工具仍经 [`run_tool`] + [`crate::tool_result::parse_legacy_output`]。
#[allow(clippy::result_large_err)]
fn run_tool_dispatch(
    name: &str,
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<(String, crate::tool_result::ParsedLegacyOutput), ToolError> {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(name) {
        return Err(ToolError::invalid_args(
            "codebase_semantic_search 需要 `fastembed` Cargo feature；当前构建未启用。".to_string(),
        ));
    }
    if find_spec(name).is_none() {
        return Err(ToolError::unknown_tool(name));
    }
    if let Some(Err(e)) = tool_args_validate::validate_parsed_str_for_builtin(name, args_json) {
        return Err(ToolError::invalid_args(e));
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
        "rust_rustc" => {
            cargo_tools::rust_rustc_try(args_json, ctx.working_dir, ctx.command_max_output_len)
                .map(|output| finish_dispatch_parsed(name, output))
        }
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
/// **`run_command`**、**`cargo_*` / `rust_test_one` / `rust_rustc`**、**`read_file`**、**`search_in_files`** 在 [`run_tool_dispatch`] 中经 `*_try` 返回显式 [`ToolError`]；其余工具仍由 [`crate::tool_result::parse_legacy_output`] 从正文推断。
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
    let parsed = crate::tool_result::parse_legacy_output("run_command", result);
    parsed.ok && parsed.exit_code == Some(0)
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

/// 仅 **`cargo test`**：重置少数进程级可变 `static`（`run_command` 秒级限流、**`test_result_cache`** LRU）。
/// 集成测试可在夹具开头调用 crate 根 **[`crate::reset_process_tool_globals_for_tests`]**。
#[cfg(test)]
pub(crate) fn reset_process_tool_globals_for_tests() {
    command::reset_run_command_rate_limit_for_tests();
    test_result_cache::reset_test_result_cache_for_tests();
}

#[cfg(test)]
#[path = "mod/tests.rs"]
mod tests;
