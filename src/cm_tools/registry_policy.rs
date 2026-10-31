//! 并行墙钟、只读判定、`SyncDefault` 内联等与 `[tool_registry]` 配置对应的策略。

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::cm_config::tool_write_effect::builtin_write_effect_tools;
use crate::cm_config::{AgentConfig, WebSearchProvider};
use crate::cm_types::ToolCall;

use crate::cm_tools::tool_dispatch::{
    HandlerId, HandlerLookupTable, ToolExecutionClass, execution_class_for_tool,
};
use crate::cm_names as tool_names;
use crate::cm_tools::tool_naming::{ToolOrigin, tool_origin};

/// `web_search`（worbrow）外圈相对配置超时的额外等待秒数，供浏览器进程收尾。
pub const WEB_SEARCH_WORBROW_OUTER_GRACE_SECS: u64 = 15;
/// Brave/Tavily 外圈相对 reqwest 超时的少量缓冲。
pub const WEB_SEARCH_API_OUTER_GRACE_SECS: u64 = 2;
/// `terminal_session` 外圈相对内层墙钟的收尾宽限（秒）：让内层先优雅返回（会话仍打开）。
pub const TERMINAL_SESSION_OUTER_GRACE_SECS: u64 = 5;

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
        WebSearchSpawnTimeout => web_search_outer_wall_secs(cfg),
        CommandSpawnTimeout => cfg.command_exec.command_timeout_secs.max(1),
        Workflow | BlockingSync => cfg.command_exec.command_timeout_secs.max(1),
    }
}

/// `web_search`：`spawn_blocking` **外圈**墙钟（秒）= 配置超时 + 收尾宽限。
///
/// 内层（worbrow / reqwest）使用 [`AgentConfig::web_search`].`web_search_timeout_secs`；
/// 外圈略长，避免外圈先到期后仍留下浏览器进程。
pub fn web_search_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    web_search_outer_wall_secs_for(
        cfg.web_search.web_search_provider,
        cfg.web_search.web_search_timeout_secs,
    )
}

/// 与 [`web_search_outer_wall_secs`] 相同算法，供 workflow 等仅持有拆开字段的路径使用。
pub fn web_search_outer_wall_secs_for(provider: WebSearchProvider, timeout_secs: u64) -> u64 {
    let inner = timeout_secs.max(1);
    let grace = match provider {
        WebSearchProvider::Worbrow => WEB_SEARCH_WORBROW_OUTER_GRACE_SECS,
        WebSearchProvider::Brave | WebSearchProvider::Tavily => WEB_SEARCH_API_OUTER_GRACE_SECS,
    };
    inner.saturating_add(grace)
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

/// `terminal_session`：`spawn_blocking` **外圈** `tokio::time::timeout`（秒）= 内层墙钟 + 收尾宽限。
///
/// 内层（`drain_until_idle`）使用 `command_exec.command_timeout_secs`；外圈略长，
/// 让内层先优雅返回（会话仍打开），仅在病态挂起时由外层超时兜底 drop future 触发会话清理。
pub fn terminal_session_outer_wall_secs(cfg: &AgentConfig) -> u64 {
    parallel_tool_wall_timeout_secs(cfg, tool_names::TERMINAL_SESSION)
        .saturating_add(TERMINAL_SESSION_OUTER_GRACE_SECS)
}

// 写效应名单唯一来源在 `crate::cm_config::tool_write_effect`（`cm_memory` 的语义索引失效
// 判定共用同一份）；本模块只负责「按配置替换」与「非内置一律不算只读」的策略。

/// 是否为**内置**工具（非 `mcp__*` / `plugins/*.json` 动态工具）。
///
/// 权限等级的只读判定只认内置工具：外部工具的可用性由 `PermissionCaps::external_tools`
/// 决定，不能被「只读正列表」顺手放行（设计 §8.1）。
#[must_use]
pub fn is_builtin_tool(name: &str) -> bool {
    tool_origin(name) == ToolOrigin::Builtin
}

/// 权限语义的「只读」判定（权限等级 L1 的只读正列表推导用）。
///
/// **不读** `tool_registry_write_effect_tools`：那是并行只读批的**性能开关**（整体替换式），
/// 运维为调并行顺手漏配 `run_command` 就会让 L1 放行子进程——不能拿它当安全边界
/// （设计 `docs/design/permission_levels.md` §4.2 / §7.2）。
///
/// 非内置来源（`mcp__*` / `plugins/*.json` 动态工具）一律**不算**只读：L1 下它们必须被
/// early-deny 拒掉，而不是靠「没进 tools 列表」兜着。
pub fn is_readonly_tool_for_permission(name: &str) -> bool {
    is_builtin_tool(name) && !builtin_write_effect_tools().contains(name)
}

/// 判断工具是否为只读（不修改工作区文件系统），供并行执行决策使用。
pub fn is_readonly_tool(cfg: &AgentConfig, name: &str) -> bool {
    if tool_origin(name) != ToolOrigin::Builtin {
        return false;
    }
    let writes = match &cfg.tool_registry_policy.tool_registry_write_effect_tools {
        None => builtin_write_effect_tools(),
        Some(arc) => arc.as_ref(),
    };
    !writes.contains(name)
}

/// 同轮「只读结果去重缓存」的豁免清单。
///
/// 这些工具虽为只读，但结果会在**同一回合内**随外部状态推进而变化
/// （如后台任务状态与输出），若命中同轮去重缓存，模型在同一轮内重复查询只会拿到过期结果。
fn builtin_dedup_cache_exempt_tools() -> &'static HashSet<String> {
    static E: OnceLock<HashSet<String>> = OnceLock::new();
    E.get_or_init(|| {
        [tool_names::BACKGROUND_JOB]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    })
}

/// 是否可参与「同轮只读结果去重缓存」（读取侧与写入侧共用同一判定）。
///
/// 只读工具默认参与；`builtin_dedup_cache_exempt_tools` 中的工具除外。
pub fn tool_output_dedup_cache_eligible(cfg: &AgentConfig, name: &str) -> bool {
    is_readonly_tool(cfg, name) && !builtin_dedup_cache_exempt_tools().contains(name)
}

fn builtin_parallel_sync_denied_exact() -> &'static HashSet<String> {
    static S: OnceLock<HashSet<String>> = OnceLock::new();
    S.get_or_init(|| {
        [
            tool_names::REPO_OVERVIEW_SWEEP,
            tool_names::CRATE_CONTRACT_MAP,
            tool_names::CODEBASE_SEMANTIC_SEARCH,
            tool_names::DOCS_HEALTH_SWEEP,
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
        "docker_",
        "podman_",
        "ruff_",
        "pytest",
        "mypy_",
        "uv_",
        "pre_commit",
        "python_",
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
    tool_origin(name) != ToolOrigin::Mcp
        && is_readonly_tool(cfg, name)
        && parallel_batch_eligible_tool(handler_lookup, cfg, name)
}

/// 本批 **至少 2 个** 工具且全部为语义只读、且均为 `parallel_batch_eligible_tool` 时，可在单轮内并行执行。
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
    // 2026-10-24：原默认两项（`get_current_time` / `convert_units`）已随工具移除，故默认为空集；
    // 仍可通过 `tool_registry_sync_default_inline_tools` 覆盖。
    S.get_or_init(HashSet::new)
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

#[cfg(test)]
mod web_search_wall_tests {
    use super::*;

    #[test]
    fn worbrow_outer_wall_adds_browser_grace() {
        assert_eq!(
            web_search_outer_wall_secs_for(WebSearchProvider::Worbrow, 60),
            60 + WEB_SEARCH_WORBROW_OUTER_GRACE_SECS
        );
        assert_eq!(
            web_search_outer_wall_secs_for(WebSearchProvider::Brave, 30),
            30 + WEB_SEARCH_API_OUTER_GRACE_SECS
        );
    }
}
