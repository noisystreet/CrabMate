//! 并行墙钟、只读判定、`SyncDefault` 内联等与 `[tool_registry]` 配置对应的策略。

use std::collections::HashSet;
use std::sync::OnceLock;

use crabmate_config::AgentConfig;
use crabmate_types::ToolCall;

use crate::tool_dispatch::{
    HandlerId, HandlerLookupTable, ToolExecutionClass, execution_class_for_tool,
};
use crate::tool_naming::{is_dynamic_tool_name, is_mcp_proxy_tool};

fn execution_class_parallel_wall_key(class: ToolExecutionClass) -> &'static str {
    match class {
        ToolExecutionClass::Workflow => "workflow",
        ToolExecutionClass::CommandSpawnTimeout => "command_spawn_timeout",
        ToolExecutionClass::WeatherSpawnTimeout => "weather_spawn_timeout",
        ToolExecutionClass::WebSearchSpawnTimeout => "web_search_spawn_timeout",
        ToolExecutionClass::HttpFetchSpawnTimeout => "http_fetch_spawn_timeout",
        ToolExecutionClass::BlockingSync => "blocking_sync",
    }
}

/// 并行只读批与 **`SyncDefault` + `spawn_blocking`** 路径共用的墙上时钟上限（秒）。
pub fn parallel_tool_wall_timeout_secs(cfg: &AgentConfig, tool_name: &str) -> u64 {
    let class = execution_class_for_tool(tool_name);
    let key = execution_class_parallel_wall_key(class);
    if let Some(&secs) = cfg
        .tool_registry_policy
        .tool_registry_parallel_wall_timeout_secs
        .get(key)
    {
        return secs.max(1);
    }
    use ToolExecutionClass::*;
    match class {
        HttpFetchSpawnTimeout => cfg
            .http_fetch
            .http_fetch_timeout_secs
            .max(1)
            .max(cfg.command_exec.command_timeout_secs.max(1)),
        WeatherSpawnTimeout => cfg.weather_tool.weather_timeout_secs.max(1),
        WebSearchSpawnTimeout => cfg.web_search.web_search_timeout_secs.max(1),
        CommandSpawnTimeout => cfg.command_exec.command_timeout_secs.max(1),
        Workflow | BlockingSync => cfg.command_exec.command_timeout_secs.max(1),
    }
}

/// `http_fetch` / `http_request`：`spawn_blocking` **外圈** `tokio::time::timeout`。
pub fn http_fetch_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_policy
        .tool_registry_http_fetch_wall_timeout_secs
        .unwrap_or_else(|| {
            cfg.command_exec
                .command_timeout_secs
                .max(cfg.http_fetch.http_fetch_timeout_secs)
                .max(1)
        })
}

pub fn http_request_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_policy
        .tool_registry_http_request_wall_timeout_secs
        .unwrap_or_else(|| http_fetch_outer_wall_secs(cfg))
}

fn builtin_write_effect_tools() -> &'static HashSet<String> {
    static W: OnceLock<HashSet<String>> = OnceLock::new();
    W.get_or_init(|| {
        [
            "create_file",
            "modify_file",
            "copy_file",
            "move_file",
            "delete_file",
            "delete_dir",
            "append_file",
            "create_dir",
            "search_replace",
            "chmod_file",
            "apply_patch",
            "format_file",
            "ast_grep_rewrite",
            "structured_patch",
            "git_stage_files",
            "git_commit",
            "git_checkout",
            "git_branch_create",
            "git_branch_delete",
            "git_push",
            "git_merge",
            "git_rebase",
            "git_stash",
            "git_tag",
            "git_reset",
            "git_cherry_pick",
            "git_revert",
            "git_clone",
            "git_remote_set_url",
            "git_apply",
            "git_fetch",
            "cargo_fix",
            "cargo_clean",
            "python_install_editable",
            "npm_install",
            "go_mod_tidy",
            "docker_build",
            "long_term_remember",
            "long_term_forget",
            "run_command",
            "terminal_session",
            "playbook_run_commands",
            "python_snippet_run",
            "run_executable",
            "workflow_execute",
            "http_request",
            "gh_api",
            "gh_pr_create",
            "gh_pr_merge",
            "gh_pr_review",
            "gh_pr_comment",
            "gh_issue_create",
            "gh_run_rerun",
            "gh_release_create",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    })
}

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
pub fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    if is_mcp_proxy_tool(name) {
        return false;
    }
    if is_dynamic_tool_name(name) {
        return false;
    }
    let writes = match &cfg.tool_registry_policy.tool_registry_write_effect_tools {
        None => builtin_write_effect_tools(),
        Some(arc) => arc.as_ref(),
    };
    !writes.contains(name)
}

fn builtin_parallel_sync_denied_exact() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| {
        [
            "rust_compiler_json",
            "quality_workspace",
            "ci_pipeline_local",
            "repo_overview_sweep",
            "crate_contract_map",
            "codebase_semantic_search",
            "docs_health_sweep",
            "playbook_run_commands",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    })
}

fn builtin_parallel_sync_prefix_hit(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "cargo_",
        "npm_",
        "frontend_",
        "go_",
        "maven_",
        "gradle_",
        "docker_",
        "podman_",
        "ruff_",
        "pytest",
        "mypy_",
        "uv_",
        "pre_commit",
        "python_",
        "typos_",
        "codespell_",
        "gh_",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

fn parallel_sync_batch_denied(cfg: &AgentConfig, name: &str) -> bool {
    let exact = match &cfg
        .tool_registry_policy
        .tool_registry_parallel_sync_denied_tools
    {
        None => builtin_parallel_sync_denied_exact(),
        Some(arc) => arc.as_ref(),
    };
    if exact.contains(name) {
        return true;
    }
    match &cfg
        .tool_registry_policy
        .tool_registry_parallel_sync_denied_prefixes
    {
        None => builtin_parallel_sync_prefix_hit(name),
        Some(prefs) => prefs.iter().any(|p| name.starts_with(p)),
    }
}

fn parallel_batch_eligible_tool(
    handler_lookup: &HandlerLookupTable,
    cfg: &AgentConfig,
    name: &str,
) -> bool {
    if parallel_sync_batch_denied(cfg, name) {
        return false;
    }
    matches!(
        handler_lookup.id_for(name),
        HandlerId::SyncDefault
            | HandlerId::HttpFetch
            | HandlerId::GetWeather
            | HandlerId::WebSearch
    )
}

/// 单工具是否满足「可与其它同类工具同批并行」的语义（不含「至少 2 个调用」前提）。
pub fn tool_ok_for_parallel_readonly_batch_piece(
    handler_lookup: &HandlerLookupTable,
    cfg: &AgentConfig,
    name: &str,
) -> bool {
    !is_mcp_proxy_tool(name)
        && is_readonly_tool(cfg, name)
        && parallel_batch_eligible_tool(handler_lookup, cfg, name)
}

/// 本批 **至少 2 个** 工具且全部为语义只读、且均为 [`parallel_batch_eligible_tool`] 时，可在单轮内并行执行。
pub fn tool_calls_allow_parallel_sync_batch(
    handler_lookup: &HandlerLookupTable,
    cfg: &AgentConfig,
    tool_calls: &[ToolCall],
) -> bool {
    tool_calls.len() > 1
        && tool_calls.iter().all(|tc| {
            tool_ok_for_parallel_readonly_batch_piece(
                handler_lookup,
                cfg,
                tc.function.name.as_str(),
            )
        })
}

fn builtin_sync_default_inline_tools() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| {
        ["get_current_time", "convert_units"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    })
}

/// 无子进程、无阻塞网络/磁盘的 `SyncDefault` 工具：跳过 `spawn_blocking`。
pub fn sync_default_runs_inline(cfg: &AgentConfig, name: &str) -> bool {
    match &cfg
        .tool_registry_policy
        .tool_registry_sync_default_inline_tools
    {
        None => builtin_sync_default_inline_tools().contains(name),
        Some(arc) => arc.contains(name),
    }
}
