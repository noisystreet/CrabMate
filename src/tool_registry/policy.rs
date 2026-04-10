//! 并行墙钟、只读判定、`SyncDefault` 内联等与 `[tool_registry]` 配置对应的策略。

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::config::AgentConfig;
use crate::types::ToolCall;

use super::meta::{HandlerId, ToolExecutionClass, execution_class_for_tool, handler_id_for};

fn execution_class_parallel_wall_key(class: ToolExecutionClass) -> &'static str {
    match class {
        ToolExecutionClass::Workflow => "workflow",
        ToolExecutionClass::CommandSpawnTimeout => "command_spawn_timeout",
        ToolExecutionClass::ExecutableSpawnTimeout => "executable_spawn_timeout",
        ToolExecutionClass::WeatherSpawnTimeout => "weather_spawn_timeout",
        ToolExecutionClass::WebSearchSpawnTimeout => "web_search_spawn_timeout",
        ToolExecutionClass::HttpFetchSpawnTimeout => "http_fetch_spawn_timeout",
        ToolExecutionClass::BlockingSync => "blocking_sync",
    }
}

/// 并行只读批与 **`SyncDefault` + `spawn_blocking`** 路径共用的墙上时钟上限（秒），与各 `execute_*_web` 中 **`tokio::time::timeout`** 一致，避免批内工具无限阻塞。
///
/// 可由 **`[tool_registry] parallel_wall_timeout_secs`** 按执行类键覆盖（见 `config/tools.toml`）。
pub fn parallel_tool_wall_timeout_secs(cfg: &AgentConfig, tool_name: &str) -> u64 {
    let class = execution_class_for_tool(tool_name);
    let key = execution_class_parallel_wall_key(class);
    if let Some(&secs) = cfg.tool_registry_parallel_wall_timeout_secs.get(key) {
        return secs.max(1);
    }
    use ToolExecutionClass::*;
    match class {
        HttpFetchSpawnTimeout => cfg
            .http_fetch_timeout_secs
            .max(1)
            .max(cfg.command_timeout_secs.max(1)),
        WeatherSpawnTimeout => cfg.weather_timeout_secs.max(1),
        WebSearchSpawnTimeout => cfg.web_search_timeout_secs.max(1),
        CommandSpawnTimeout | ExecutableSpawnTimeout => cfg.command_timeout_secs.max(1),
        Workflow | BlockingSync => cfg.command_timeout_secs.max(1),
    }
}

/// `http_fetch` / `http_request`：`spawn_blocking` **外圈** `tokio::time::timeout`（与 `reqwest` 内读秒数 `http_fetch_timeout_secs` 区分）。
pub(crate) fn http_fetch_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_http_fetch_wall_timeout_secs
        .unwrap_or_else(|| {
            cfg.command_timeout_secs
                .max(cfg.http_fetch_timeout_secs)
                .max(1)
        })
}

pub(crate) fn http_request_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    cfg.tool_registry_http_request_wall_timeout_secs
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
            "run_command",
            "playbook_run_commands",
            "python_snippet_run",
            "run_executable",
            "workflow_execute",
            "http_request",
            "gh_api",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    })
}

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
/// 写操作工具（create/modify/delete/move/copy/format/apply_patch 等）及带审批的工具返回 false。
///
/// 写工具名表可由 **`[tool_registry] write_effect_tools`** 整表覆盖。
pub fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    if crate::mcp::is_mcp_proxy_tool(name) {
        // 外部 MCP 工具语义未知，禁止与内建只读工具并行同批执行。
        return false;
    }
    let writes = match &cfg.tool_registry_write_effect_tools {
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
    name.starts_with("cargo_")
        || name.starts_with("npm_")
        || name.starts_with("frontend_")
        || name.starts_with("go_")
        || name.starts_with("maven_")
        || name.starts_with("gradle_")
        || name.starts_with("docker_")
        || name.starts_with("podman_")
        || name.starts_with("ruff_")
        || name.starts_with("pytest")
        || name.starts_with("mypy_")
        || name.starts_with("uv_")
        || name.starts_with("pre_commit")
        || name.starts_with("python_")
        || name.starts_with("typos_")
        || name.starts_with("codespell_")
        || name.starts_with("gh_")
}

/// 即使 [`is_readonly_tool`] 为真，并行 `spawn_blocking` 仍可能争抢 cargo/npm 等构建锁或缓存；勿与同批其它工具并行。
fn parallel_sync_batch_denied(cfg: &AgentConfig, name: &str) -> bool {
    let exact = match &cfg.tool_registry_parallel_sync_denied_tools {
        None => builtin_parallel_sync_denied_exact(),
        Some(arc) => arc.as_ref(),
    };
    if exact.contains(name) {
        return true;
    }
    match &cfg.tool_registry_parallel_sync_denied_prefixes {
        None => builtin_parallel_sync_prefix_hit(name),
        Some(prefs) => prefs.iter().any(|p| name.starts_with(p)),
    }
}

/// 可与其它只读工具同批 **并行** 执行的工具（不含 `http_request`、命令类、MCP）。
///
/// - **`SyncDefault`**：内建只读且非 `parallel_sync_batch_denied`。
/// - **`http_fetch`**：GET/HEAD 只读；审批在并行 `spawn_blocking` 之前**串行**完成（见 `execute_tools`）。
/// - **`get_weather` / `web_search`**：出站只读 HTTP；无工作区副作用，可与 `read_file` 等同批并行。
fn parallel_batch_eligible_tool(cfg: &AgentConfig, name: &str) -> bool {
    if parallel_sync_batch_denied(cfg, name) {
        return false;
    }
    matches!(
        handler_id_for(name),
        HandlerId::SyncDefault
            | HandlerId::HttpFetch
            | HandlerId::GetWeather
            | HandlerId::WebSearch
    )
}

/// 单工具是否满足「可与其它同类工具同批并行」的语义（不含「至少 2 个调用」前提）。
///
/// 与 [`tool_calls_allow_parallel_sync_batch`] 中每个 `ToolCall` 的判定一致；供分阶段规划**优化轮**提示词列举可批量并行的内建工具名。
pub fn tool_ok_for_parallel_readonly_batch_piece(cfg: &AgentConfig, name: &str) -> bool {
    !crate::mcp::is_mcp_proxy_tool(name)
        && is_readonly_tool(cfg, name)
        && parallel_batch_eligible_tool(cfg, name)
}

/// 本批 **至少 2 个** 工具且全部为语义只读、且均为 [`parallel_batch_eligible_tool`] 时，可在单轮内并行执行
///（`SyncDefault` / `http_fetch` / `get_weather` / `web_search`；**不含** `http_request`、命令类、MCP；`http_fetch` 的审批先于并行 IO，见 `agent_turn::per_execute_tools_common`）。
pub fn tool_calls_allow_parallel_sync_batch(cfg: &AgentConfig, tool_calls: &[ToolCall]) -> bool {
    tool_calls.len() > 1
        && tool_calls
            .iter()
            .all(|tc| tool_ok_for_parallel_readonly_batch_piece(cfg, tc.function.name.as_str()))
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

/// 无子进程、无阻塞网络/磁盘的 `SyncDefault` 工具：跳过 `spawn_blocking`，以免线程池调度开销大于工具本身。
///
/// 可由 **`[tool_registry] sync_default_inline_tools`** 覆盖。
pub(crate) fn sync_default_runs_inline(cfg: &AgentConfig, name: &str) -> bool {
    match &cfg.tool_registry_sync_default_inline_tools {
        None => builtin_sync_default_inline_tools().contains(name),
        Some(arc) => arc.contains(name),
    }
}
