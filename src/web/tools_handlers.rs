//! `GET /tools`、`GET /tools/{tool_name}`、`PUT /tools/{tool_name}/enabled`、`GET /tools/plugins`
//! handler。
//!
//! 契约真源：`docs/design/tool_management_api.md`（P0 + P1，作用域进程级）。
//! 只读面**绝不建立 MCP 会话**：MCP 源仅读已缓存会话快照与已配置 server 列表。

use std::collections::HashMap;
use std::path::Path;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use super::app_state::AppStateHttpCore;
use super::http_types::chat::ApiError;
use super::http_types::tools::{
    DynamicToolFileDetail, DynamicToolFileView, PluginsListResponse, ToolDetailView,
    ToolEnabledBody, ToolEnabledResponse, ToolListItem, ToolPolicyView, ToolSourceCounts,
    ToolsListResponse,
};
use crate::cm_names as tool_names;
use crate::cm_api_contract::error_codes;
use crate::cm_tools::registry_policy;
use crate::cm_tools::tool_dispatch::HandlerLookupTable;
use crate::cm_tools::tool_naming;
use crate::cm_tools::tool_retry_policy::ToolRetrySpec;
use crate::cm_tools::tools::{builtin_tool_descriptors, cached_params_for_tool_name};
use crate::config::AgentConfig;

type ApiErr = (StatusCode, Json<ApiError>);

fn err(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiErr {
    (status, Json(ApiError::new(code, message)))
}

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// `GET /tools` 查询参数；`source` 可重复。
#[derive(Debug, Deserialize)]
pub(crate) struct ToolsQuery {
    #[serde(default)]
    source: Vec<String>,
    category: Option<String>,
    dev_tag: Option<String>,
    q: Option<String>,
    /// 字符串接收以便对非法值统一返回 `INVALID_TOOL_FILTER`。
    limit: Option<String>,
    offset: Option<String>,
}

/// 单条工具的内部投影（枚举后、过滤后的统一形态）。
struct ToolEntry {
    name: String,
    description: String,
    category: &'static str,
    source: &'static str,
    dev_tags: Vec<String>,
    present: bool,
    absent_reason: Option<&'static str>,
}

/// 工具名定位参数校验：长度 ≤ 128 且字符集 `[A-Za-z0-9_.:-]`。
fn validate_tool_name(name: &str) -> Result<(), ApiErr> {
    let ok = !name.is_empty()
        && name.chars().count() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'));
    if ok {
        Ok(())
    } else {
        Err(err(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_TOOL_NAME,
            format!("工具名不合法：{name}"),
        ))
    }
}

/// 动态工具文件名守卫：须为 `<stem>.json`，`stem` 非空且仅含 `[A-Za-z0-9_-]`。
///
/// 显式拒绝路径分隔符、`..` 等（对齐 `/workspace/file` 风格），不做任何规范化。
fn validate_plugin_file_name(file: &str) -> Result<(), ApiErr> {
    let stem_ok = file.strip_suffix(".json").is_some_and(|stem| {
        !stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    });
    if stem_ok {
        Ok(())
    } else {
        Err(err(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_PLUGIN_DEFINITION,
            format!("动态工具文件名不合法：{file}（须为 [A-Za-z0-9_-]+.json）"),
        ))
    }
}

/// 命中 `[tool_registry] disabled_tools`（精确）或 `disabled_tool_prefixes`（前缀）。
fn policy_disabled(cfg: &AgentConfig, name: &str) -> bool {
    let p = &cfg.tool_registry_policy;
    if let Some(disabled) = p.tool_registry_disabled_tools.as_ref()
        && disabled.contains(name)
    {
        return true;
    }
    if let Some(prefixes) = p.tool_registry_disabled_tool_prefixes.as_ref()
        && prefixes
            .iter()
            .any(|x| !x.is_empty() && name.starts_with(x.as_str()))
    {
        return true;
    }
    false
}

/// 既有配置硬编码特判（`codebase_semantic_search` / `long_term_memory_*`）。
fn config_disabled(cfg: &AgentConfig, name: &str) -> bool {
    if name == tool_names::CODEBASE_SEMANTIC_SEARCH
        && !cfg.codebase_semantic.codebase_semantic_search_enabled
    {
        return true;
    }
    if matches!(
        name,
        tool_names::LONG_TERM_REMEMBER
            | tool_names::LONG_TERM_FORGET
            | tool_names::LONG_TERM_MEMORY_LIST
    ) && !cfg.long_term_memory.long_term_memory_enabled
    {
        return true;
    }
    false
}

/// 内置工具的在场/缺席原因（`feature_not_built` → `config_disabled` → `disabled_by_policy`）。
fn builtin_absent_reason(
    cfg: &AgentConfig,
    name: &str,
    requires_fastembed: bool,
) -> Option<&'static str> {
    if requires_fastembed && !cfg!(feature = "fastembed") {
        return Some("feature_not_built");
    }
    if config_disabled(cfg, name) {
        return Some("config_disabled");
    }
    if policy_disabled(cfg, name) {
        return Some("disabled_by_policy");
    }
    None
}

/// 三源枚举（只读）。持 `cfg` 读锁期间不再次读锁 `cfg`（调用方须先取工作区串）。
async fn enumerate_tools_from(cfg: &AgentConfig, ws: &str) -> Vec<ToolEntry> {
    let mut out = Vec::new();

    for d in builtin_tool_descriptors() {
        let reason = builtin_absent_reason(cfg, d.name, d.requires_fastembed);
        out.push(ToolEntry {
            name: d.name.to_string(),
            description: d.description.to_string(),
            category: d.category.as_str(),
            source: "builtin",
            dev_tags: d.dev_tags.iter().map(|s| s.to_string()).collect(),
            present: reason.is_none(),
            absent_reason: reason,
        });
    }

    if !ws.trim().is_empty() {
        for t in crate::cm_internal::dynamic_tools::load_dynamic_tools(Path::new(ws)) {
            let reason = policy_disabled(cfg, &t.function.name).then_some("disabled_by_policy");
            out.push(ToolEntry {
                name: t.function.name,
                description: t.function.description,
                category: "development",
                source: "dynamic",
                dev_tags: Vec::new(),
                present: reason.is_none(),
                absent_reason: reason,
            });
        }
    }

    let resolved = crate::mcp::resolve_mcp_config(cfg);
    for server in crate::mcp::mcp_servers_runtime_status(&resolved).await {
        let mut desc_by_remote: HashMap<&str, Option<&str>> = HashMap::new();
        for rt in &server.remote_tools {
            desc_by_remote.insert(rt.name.as_str(), rt.description.as_deref());
        }
        for openai_name in &server.openai_tool_names {
            let remote = crate::mcp::parse_mcp_openai_tool_name(openai_name).map(|(_, r)| r);
            let description = remote
                .as_deref()
                .and_then(|r| desc_by_remote.get(r).cloned().flatten())
                .unwrap_or_default()
                .to_string();
            let reason = policy_disabled(cfg, openai_name).then_some("disabled_by_policy");
            out.push(ToolEntry {
                name: openai_name.clone(),
                description,
                category: "development",
                source: "mcp",
                dev_tags: Vec::new(),
                present: reason.is_none(),
                absent_reason: reason,
            });
        }
    }

    out
}

/// 工具在已枚举集合中的 `source`（未注册返回 `None`）。
fn entry_source(entries: &[ToolEntry], name: &str) -> Option<&'static str> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.source)
}

fn parse_enum_filters(query: &ToolsQuery) -> Result<Option<Vec<&'static str>>, ApiErr> {
    let mut sources = Vec::new();
    for s in &query.source {
        let v = match s.as_str() {
            "builtin" => "builtin",
            "dynamic" => "dynamic",
            "mcp" => "mcp",
            other => {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    error_codes::INVALID_TOOL_FILTER,
                    format!("不支持的 source 取值：{other}（仅 builtin/dynamic/mcp）"),
                ));
            }
        };
        sources.push(v);
    }
    if sources.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sources))
    }
}

fn parse_category_filter(query: &ToolsQuery) -> Result<Option<&'static str>, ApiErr> {
    match query.category.as_deref() {
        None => Ok(None),
        Some("basic") => Ok(Some("basic")),
        Some("development") => Ok(Some("development")),
        Some(other) => Err(err(
            StatusCode::BAD_REQUEST,
            error_codes::INVALID_TOOL_FILTER,
            format!("不支持的 category 取值：{other}（仅 basic/development）"),
        )),
    }
}

fn parse_usize_or(raw: Option<&String>, default: usize) -> Result<usize, ApiErr> {
    match raw {
        None => Ok(default),
        Some(s) => s.trim().parse::<usize>().map_err(|_| {
            err(
                StatusCode::BAD_REQUEST,
                error_codes::INVALID_TOOL_FILTER,
                format!("非法数值参数：{s}"),
            )
        }),
    }
}

/// `GET /tools` / `GET /tools/{tool_name}` 的等级基准：**进程默认档**下的六轴投影
/// （设计 `permission_levels.md` §16.4）。
///
/// 这两个端点没有会话也没有请求体，故 `requested` / `persisted` / `role_max` 全为 `None`，
/// min 链退化成 `min(permission_level, ceiling)`——正好是「不指定会话时这台机器能用到哪一档」。
///
/// 复用 `resolve_permission_level_for_turn` + `project_caps` 而非自己比大小，
/// 避免 `available_at_level` 与回合实际可见性长出第二套判据。
fn default_projection_caps(cfg: &AgentConfig) -> crate::cm_agent::permission::PermissionCaps {
    let resolved = crate::cm_agent::permission::resolve_permission_level_for_turn(
        crate::cm_agent::permission::PermissionTurnInput {
            cfg,
            requested: None,
            persisted: None,
            role_max: None,
            // 会话模式是**会话**轴，这里刻意给 `Act`：本函数只读 `projection_level`，
            // 它不含会话模式钳制，故这个值不影响结果。显式写 `Act`（而不是
            // `default_session_mode`）是为了让一件事在 diff 里看得见——若哪天改成读
            // `resolved.effective`，进程级清单就会随默认会话模式悄悄变成只读。
            session_mode: crate::cm_types::SessionMode::Act,
        },
    );
    crate::cm_agent::permission::project_caps(resolved.projection_level, cfg)
}

/// 单条枚举结果 → 对外投影。
///
/// 抽成纯函数是为了让 `available_at_level` 的**接线**可被单测直接钉住：断言走的就是
/// handler 调用的这个函数，而不是在测试里把它的内部逻辑重抄一遍（那样把 handler 里的
/// 赋值改成常量也照样全绿）。
fn tool_list_item(
    entry: ToolEntry,
    caps: &crate::cm_agent::permission::PermissionCaps,
) -> ToolListItem {
    // 须在 `entry.name` 被 move 进结构体之前算：判据吃的是 `&str`。
    let available_at_level =
        crate::cm_agent::permission::tool_allowed_by_caps(caps, &entry.name);
    ToolListItem {
        name: entry.name,
        description: entry.description,
        category: entry.category.to_string(),
        source: entry.source.to_string(),
        dev_tags: entry.dev_tags,
        present: entry.present,
        absent_reason: entry.absent_reason.map(|s| s.to_string()),
        available_at_level,
    }
}

/// `GET /tools`：三源只读清单（只读、不建 MCP 会话）。
pub(crate) async fn tools_list_handler(
    State(http): State<AppStateHttpCore>,
    Query(query): Query<ToolsQuery>,
) -> Result<Json<ToolsListResponse>, ApiErr> {
    let source_filter = parse_enum_filters(&query)?;
    let category_filter = parse_category_filter(&query)?;
    let limit = parse_usize_or(query.limit.as_ref(), DEFAULT_LIMIT)?.clamp(1, MAX_LIMIT);
    let offset = parse_usize_or(query.offset.as_ref(), 0)?;

    let ws = http.effective_workspace_path().await;
    let cfg = http.cfg.read().await;
    // 等级投影与工具枚举共用同一次读锁：两份投影若取到不同版本的 `cfg`，
    // 就会出现「工具按旧等级算出不可用、却按新配置列出来」的错位。
    let caps = default_projection_caps(&cfg);
    let mut entries = enumerate_tools_from(&cfg, &ws).await;
    drop(cfg);

    if let Some(sources) = &source_filter {
        entries.retain(|e| sources.contains(&e.source));
    }
    if let Some(category) = category_filter {
        entries.retain(|e| e.category == category);
    }
    if let Some(tag) = query.dev_tag.as_deref().filter(|s| !s.is_empty()) {
        entries.retain(|e| e.dev_tags.iter().any(|t| t == tag));
    }
    if let Some(q) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let needle = q.to_lowercase();
        entries.retain(|e| {
            e.name.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
        });
    }

    let counts = ToolSourceCounts {
        builtin: entries.iter().filter(|e| e.source == "builtin").count(),
        dynamic: entries.iter().filter(|e| e.source == "dynamic").count(),
        mcp: entries.iter().filter(|e| e.source == "mcp").count(),
        present: entries.iter().filter(|e| e.present).count(),
    };
    let total = entries.len();
    let tools: Vec<ToolListItem> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|e| tool_list_item(e, &caps))
        .collect();
    let has_more = offset.saturating_add(limit) < total;

    Ok(Json(ToolsListResponse {
        tools,
        total,
        limit,
        offset,
        has_more,
        counts,
        // 基准档必须回传：只有 `available_at_level` 而没有「按哪一档算的」，
        // 客户端看到 `false` 也没法解释（设计 §16.4）。
        permission_level: caps.level.as_str().to_string(),
        workspace_root: (!ws.trim().is_empty()).then_some(ws),
    }))
}

/// 单工具详情用的 `parameters`（内置走编译期缓存；动态工具读盘；MCP 无静态 schema）。
fn parameters_for(cfg: &AgentConfig, ws: &str, name: &str, source: &str) -> Value {
    match source {
        "builtin" => cached_params_for_tool_name(name).unwrap_or(Value::Null),
        "dynamic" if !ws.trim().is_empty() => crate::cm_internal::dynamic_tools::load_dynamic_tools(
            Path::new(ws),
        )
        .into_iter()
        .find(|t| t.function.name == name)
        .map(|t| t.function.parameters)
        .unwrap_or(Value::Null),
        _ => {
            let _ = cfg;
            Value::Null
        }
    }
}

/// 由已公开的 `registry_policy` 判定派生策略投影。
fn policy_view(cfg: &AgentConfig, name: &str) -> ToolPolicyView {
    let handlers = HandlerLookupTable::default_dispatch();
    let read_only = registry_policy::is_readonly_tool(cfg, name);
    let is_external = tool_naming::tool_origin(name) != tool_naming::ToolOrigin::Builtin;
    let p = &cfg.tool_registry_policy;
    let mut sub_agent_extra_allow: Vec<String> = Vec::new();
    if p.tool_registry_sub_agent_patch_write_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
    {
        sub_agent_extra_allow.push("sub_agent_patch_write_extra_tools".to_string());
    }
    if p.tool_registry_sub_agent_test_runner_extra_tools
        .as_ref()
        .is_some_and(|s| s.contains(name))
    {
        sub_agent_extra_allow.push("sub_agent_test_runner_extra_tools".to_string());
    }
    ToolPolicyView {
        read_only,
        write_effect: !is_external && !read_only,
        parallel_readonly_batch_allowed: registry_policy::tool_ok_for_parallel_readonly_batch_piece(
            &handlers, cfg, name,
        ),
        sync_default_inline: registry_policy::sync_default_runs_inline(cfg, name),
        wall_timeout_secs: registry_policy::parallel_tool_wall_timeout_secs(cfg, name),
        sub_agent_extra_allow,
        background_job_capable: p.tool_registry_background_jobs_enabled
            && p.tool_registry_background_job_async_tools.contains(name),
        retry_eligible: ToolRetrySpec::from_config(cfg).tool_retry_eligible(cfg, name, ""),
    }
}

/// `GET /tools/{tool_name}`：单工具详情 + 策略投影。
pub(crate) async fn tool_detail_handler(
    State(http): State<AppStateHttpCore>,
    AxumPath(tool_name): AxumPath<String>,
) -> Result<Json<ToolDetailView>, ApiErr> {
    validate_tool_name(&tool_name)?;
    let ws = http.effective_workspace_path().await;
    let cfg = http.cfg.read().await;
    // 与清单同一档、同一次读锁：两个端点的 `available_at_level` 必须能对得上，
    // 否则客户端会在清单与详情页看到互相矛盾的两个答案。
    let caps = default_projection_caps(&cfg);
    let entries = enumerate_tools_from(&cfg, &ws).await;
    let Some(entry) = entries.into_iter().find(|e| e.name == tool_name) else {
        return Err(err(
            StatusCode::NOT_FOUND,
            error_codes::TOOL_NOT_FOUND,
            format!("未注册的工具：{tool_name}"),
        ));
    };
    let available_at_level = crate::cm_agent::permission::tool_allowed_by_caps(&caps, &entry.name);
    let parameters = parameters_for(&cfg, &ws, &entry.name, entry.source);
    let policy = policy_view(&cfg, &entry.name);
    drop(cfg);

    Ok(Json(ToolDetailView {
        name: entry.name,
        description: entry.description,
        category: entry.category.to_string(),
        source: entry.source.to_string(),
        dev_tags: entry.dev_tags,
        present: entry.present,
        absent_reason: entry.absent_reason.map(|s| s.to_string()),
        available_at_level,
        parameters,
        policy,
    }))
}

/// `PUT /tools/{tool_name}/enabled`：写 `tool_overrides.json` 并就地生效。
pub(crate) async fn tool_enabled_handler(
    State(http): State<AppStateHttpCore>,
    AxumPath(tool_name): AxumPath<String>,
    Json(body): Json<ToolEnabledBody>,
) -> Result<Json<ToolEnabledResponse>, ApiErr> {
    validate_tool_name(&tool_name)?;

    let source = {
        let ws = http.effective_workspace_path().await;
        let cfg = http.cfg.read().await;
        let entries = enumerate_tools_from(&cfg, &ws).await;
        match entry_source(&entries, &tool_name) {
            Some(s) => s,
            None => {
                return Err(err(
                    StatusCode::NOT_FOUND,
                    error_codes::TOOL_NOT_FOUND,
                    format!("未注册的工具：{tool_name}"),
                ));
            }
        }
    };

    // 幂等：始终写入显式 bool（不删键）。
    let mut overrides = crate::user_data::load_tool_overrides();
    overrides.tools.insert(tool_name.clone(), body.enabled);
    crate::user_data::save_tool_overrides(&overrides).map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            error_codes::INTERNAL_ERROR,
            format!("写入 tool_overrides.json 失败：{e}"),
        )
    })?;

    // 就地更新运行中 cfg，令 D1 立即反映（同时落盘保证 reload/重启生效）。
    {
        let mut cfg = http.cfg.write().await;
        crate::user_data::apply_user_data_tool_overrides(&mut cfg);
    }

    // 按 D1 口径重算最终生效态：`tool_overrides.json` 只解除精确集，
    // 若该名仍被 `disabled_tool_prefixes` 命中，则如实回报 `present:false`（而非谎报生效）。
    let ws = http.effective_workspace_path().await;
    let (present, absent_reason) = {
        let cfg = http.cfg.read().await;
        match enumerate_tools_from(&cfg, &ws)
            .await
            .into_iter()
            .find(|e| e.name == tool_name)
        {
            Some(e) => (e.present, e.absent_reason),
            None => (false, None),
        }
    };

    Ok(Json(ToolEnabledResponse {
        name: tool_name,
        enabled: body.enabled,
        source: source.to_string(),
        effective_after_reload: true,
        present,
        absent_reason: absent_reason.map(|s| s.to_string()),
    }))
}

/// `GET /tools/plugins`：工作区 `plugins/*.json` 只读列表（含校验失败原因）。
pub(crate) async fn tools_plugins_handler(
    State(http): State<AppStateHttpCore>,
) -> Json<PluginsListResponse> {
    let ws = http.effective_workspace_path().await;
    if ws.trim().is_empty() {
        return Json(PluginsListResponse {
            plugins: Vec::new(),
            total: 0,
        });
    }
    let allowed: Vec<String> = {
        let cfg = http.cfg.read().await;
        cfg.command_exec.allowed_commands.to_vec()
    };
    let probes = crate::cm_internal::dynamic_tools::probe_dynamic_tool_files(
        Path::new(&ws),
        &allowed,
    );
    let plugins: Vec<DynamicToolFileView> = probes
        .into_iter()
        .map(|p| DynamicToolFileView {
            file: p.file,
            name: p.name,
            description: p.description,
            valid: p.valid,
            error: p.error,
            command_allowed: p.command_allowed,
        })
        .collect();
    let total = plugins.len();
    Json(PluginsListResponse { plugins, total })
}

/// `GET /tools/plugins/{file}`：工作区动态工具单文件只读详情（`parameters`/`args`/`pass_args_json`）。
pub(crate) async fn tool_plugin_detail_handler(
    State(http): State<AppStateHttpCore>,
    AxumPath(file): AxumPath<String>,
) -> Result<Json<DynamicToolFileDetail>, ApiErr> {
    validate_plugin_file_name(&file)?;
    let ws = http.effective_workspace_path().await;
    if ws.trim().is_empty() {
        return Err(err(
            StatusCode::NOT_FOUND,
            error_codes::PLUGIN_NOT_FOUND,
            format!("未找到动态工具文件：{file}（未设置工作区）"),
        ));
    }
    let allowed: Vec<String> = {
        let cfg = http.cfg.read().await;
        cfg.command_exec.allowed_commands.to_vec()
    };
    let Some(p) =
        crate::cm_internal::dynamic_tools::probe_dynamic_tool_file(Path::new(&ws), &file, &allowed)
    else {
        return Err(err(
            StatusCode::NOT_FOUND,
            error_codes::PLUGIN_NOT_FOUND,
            format!("未找到动态工具文件：{file}"),
        ));
    };
    Ok(Json(DynamicToolFileDetail {
        file: p.file,
        name: p.name,
        description: p.description,
        valid: p.valid,
        error: p.error,
        command_allowed: p.command_allowed,
        parameters: p.parameters,
        args: p.args,
        pass_args_json: p.pass_args_json,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    use crate::cm_config::load_config;

    /// `ApiErr` 不实现 `Debug`，用无约束助手取出 `Ok` / `Err`。
    fn ok<T>(r: Result<T, ApiErr>) -> T {
        match r {
            Ok(v) => v,
            Err(_) => panic!("expected Ok, got Err"),
        }
    }

    fn err_of<T>(r: Result<T, ApiErr>) -> ApiErr {
        match r {
            Ok(_) => panic!("expected Err, got Ok"),
            Err(e) => e,
        }
    }

    fn base_cfg() -> AgentConfig {
        load_config(None).expect("embed default")
    }

    fn query(sources: &[&str]) -> ToolsQuery {
        ToolsQuery {
            source: sources.iter().map(|s| (*s).to_string()).collect(),
            category: None,
            dev_tag: None,
            q: None,
            limit: None,
            offset: None,
        }
    }

    #[test]
    fn validate_tool_name_accepts_and_rejects() {
        assert!(validate_tool_name("run_command").is_ok());
        assert!(validate_tool_name("mcp__srv__remote:tag-1.2").is_ok());
        assert!(validate_tool_name(&"a".repeat(128)).is_ok());
        assert!(validate_tool_name("").is_err());
        assert!(validate_tool_name("has space").is_err());
        assert!(validate_tool_name("slash/name").is_err());
        assert!(validate_tool_name(&"a".repeat(129)).is_err());
        let (status, json) = err_of(validate_tool_name("bad name"));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json.0.code, error_codes::INVALID_TOOL_NAME);
    }

    #[test]
    fn validate_plugin_file_name_accepts_and_rejects() {
        assert!(validate_plugin_file_name("echo_tool.json").is_ok());
        assert!(validate_plugin_file_name("A-1_b.json").is_ok());
        assert!(validate_plugin_file_name("noext").is_err());
        assert!(validate_plugin_file_name(".json").is_err());
        assert!(validate_plugin_file_name("a/b.json").is_err());
        assert!(validate_plugin_file_name("../a.json").is_err());
        assert!(validate_plugin_file_name("a b.json").is_err());
        assert!(validate_plugin_file_name("a.JSON").is_err());
        let (status, json) = err_of(validate_plugin_file_name("../a.json"));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json.0.code, error_codes::INVALID_PLUGIN_DEFINITION);
    }

    #[test]
    fn parse_enum_filters_ok_empty_and_unknown() {
        assert_eq!(
            ok(parse_enum_filters(&query(&["builtin", "mcp"]))),
            Some(vec!["builtin", "mcp"])
        );
        assert_eq!(ok(parse_enum_filters(&query(&[]))), None);
        let (status, json) = err_of(parse_enum_filters(&query(&["nope"])));
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json.0.code, error_codes::INVALID_TOOL_FILTER);
    }

    #[test]
    fn parse_category_filter_ok_none_and_unknown() {
        assert_eq!(ok(parse_category_filter(&query(&[]))), None);
        let mut basic = query(&[]);
        basic.category = Some("basic".into());
        assert_eq!(ok(parse_category_filter(&basic)), Some("basic"));
        let mut bad = query(&[]);
        bad.category = Some("weird".into());
        let (_, json) = err_of(parse_category_filter(&bad));
        assert_eq!(json.0.code, error_codes::INVALID_TOOL_FILTER);
    }

    #[test]
    fn parse_usize_or_default_ok_and_invalid() {
        assert_eq!(ok(parse_usize_or(None, 50)), 50);
        assert_eq!(ok(parse_usize_or(Some(&"7".to_string()), 50)), 7);
        assert_eq!(ok(parse_usize_or(Some(&" 3 ".to_string()), 50)), 3);
        let (_, json) = err_of(parse_usize_or(Some(&"abc".to_string()), 50));
        assert_eq!(json.0.code, error_codes::INVALID_TOOL_FILTER);
    }

    #[test]
    fn policy_disabled_exact_and_prefix() {
        let mut cfg = base_cfg();
        assert!(!policy_disabled(&cfg, "run_command"));

        cfg.tool_registry_policy.tool_registry_disabled_tools =
            Some(Arc::new(HashSet::from(["run_command".to_string()])));
        assert!(policy_disabled(&cfg, "run_command"));
        assert!(!policy_disabled(&cfg, "run_command_other"));

        cfg.tool_registry_policy.tool_registry_disabled_tool_prefixes =
            Some(Arc::from(vec!["mcp__".to_string(), String::new()]));
        assert!(policy_disabled(&cfg, "mcp__srv__remote"));
        // 空前缀不得命中所有工具。
        assert!(!policy_disabled(&cfg, "read_file"));
    }

    #[test]
    fn builtin_absent_reason_priority() {
        let mut cfg = base_cfg();
        // 未命中任何禁用条件 -> 在场。
        assert_eq!(builtin_absent_reason(&cfg, "read_file", false), None);

        // config_disabled：长期记忆关闭时的三个工具。
        cfg.long_term_memory.long_term_memory_enabled = false;
        assert_eq!(
            builtin_absent_reason(&cfg, "long_term_memory_list", false),
            Some("config_disabled")
        );

        // disabled_by_policy：策略禁用。
        cfg.tool_registry_policy.tool_registry_disabled_tools =
            Some(Arc::new(HashSet::from(["read_file".to_string()])));
        assert_eq!(
            builtin_absent_reason(&cfg, "read_file", false),
            Some("disabled_by_policy")
        );

        // feature_not_built 优先于其它原因（仅在未启用 fastembed 的构建下断言）。
        if !cfg!(feature = "fastembed") {
            assert_eq!(
                builtin_absent_reason(&base_cfg(), "codebase_semantic_search", true),
                Some("feature_not_built")
            );
        }
    }

    #[tokio::test]
    async fn enumerate_empty_workspace_has_no_dynamic_and_finds_builtin() {
        let cfg = base_cfg();
        let entries = enumerate_tools_from(&cfg, "").await;

        // 无工作区：dynamic 源为空（不得因此报错或返回其它源）。
        assert!(entries.iter().all(|e| e.source != "dynamic"));
        assert!(entries.iter().any(|e| e.source == "builtin"));
        assert_eq!(entry_source(&entries, "run_command"), Some("builtin"));
        assert_eq!(entry_source(&entries, "definitely_not_a_tool"), None);
    }

    /// 测试用最小 `ToolEntry`（来源固定 `builtin`；`present` 与 `absent_reason` 自洽）。
    fn entry(name: &str, present: bool) -> ToolEntry {
        ToolEntry {
            name: name.to_string(),
            description: String::new(),
            category: "basic",
            source: "builtin",
            dev_tags: Vec::new(),
            present,
            absent_reason: (!present).then_some("disabled_by_policy"),
        }
    }

    /// 指定等级下的 caps（上限拉到顶，确保测的是等级本身而不是 ceiling 推导）。
    fn caps_at(
        level: crate::cm_types::PermissionLevel,
    ) -> crate::cm_agent::permission::PermissionCaps {
        let mut cfg = base_cfg();
        cfg.permission.permission_level = level;
        cfg.permission.permission_level_max_effective =
            crate::cm_types::PermissionLevel::Unrestricted;
        default_projection_caps(&cfg)
    }

    /// `available_at_level` 的**接线**（§16.4）：五档 × 三类工具矩阵。
    ///
    /// 断言必须经 `tool_list_item`——即 handler 真正调用的那个函数。若在测试里直接把
    /// `tool_allowed_by_caps` 与 caps 组合，等价于把 handler 的内部逻辑重抄一遍，
    /// 于是把结构体里的赋值改成常量也照样全绿。
    #[test]
    fn tool_list_item_reports_availability_per_level() {
        use crate::cm_types::PermissionLevel;

        let available = |level: PermissionLevel, name: &str| {
            tool_list_item(entry(name, true), &caps_at(level)).available_at_level
        };

        // L0：无工具。
        for name in ["read_file", "modify_file", "run_command"] {
            assert!(!available(PermissionLevel::Chat, name), "L0 不应允许 {name}");
        }
        // L1：只读正列表——写工具与命令类都不可用。
        assert!(available(PermissionLevel::Read, "read_file"));
        assert!(!available(PermissionLevel::Read, "modify_file"));
        assert!(!available(PermissionLevel::Read, "run_command"));
        // L2–L4：全量可见（外部工具轴仍是 Denied，但内置写工具与命令类可用）。
        for level in [
            PermissionLevel::Workspace,
            PermissionLevel::Approved,
            PermissionLevel::Unrestricted,
        ] {
            for name in ["read_file", "modify_file", "run_command"] {
                assert!(available(level, name), "{level:?} 下 {name} 应可用");
            }
        }
    }

    /// `present` 与 `available_at_level` 是两条独立的轴，且都真的出现在 JSON 里。
    ///
    /// 「在场但当前等级调不动」只能靠两个字段并列表达；字段漏标 / 改名时客户端只会
    /// 看到「没有这个字段」而不是报错，所以这里直接断言序列化结果。
    #[test]
    fn present_and_available_at_level_are_independent_axes() {
        use crate::cm_types::PermissionLevel;

        let item = tool_list_item(entry("modify_file", true), &caps_at(PermissionLevel::Read));
        assert!(item.present, "写工具在 L1 下仍然在场（可被注册）");
        assert!(!item.available_at_level, "但 L1 下调不动");

        let v = serde_json::to_value(&item).expect("ToolListItem 必须可序列化");
        assert_eq!(v.get("present"), Some(&Value::Bool(true)));
        assert_eq!(v.get("available_at_level"), Some(&Value::Bool(false)));
    }

    /// 基准取 `min(permission_level, ceiling)`：调低上限后，清单立刻跟着变窄。
    ///
    /// 这条钉住「`GET /tools` 是投影而不是配置回显」——热重载把
    /// `permission_level_max` 降下来（只允许降）后，客户端不该还看到一批调不动的工具。
    #[test]
    fn ceiling_clamps_default_projection_caps() {
        use crate::cm_types::PermissionLevel;

        let mut cfg = base_cfg();
        cfg.permission.permission_level = PermissionLevel::Unrestricted;
        cfg.permission.permission_level_max_effective = PermissionLevel::Read;
        let caps = default_projection_caps(&cfg);

        // 回传给客户端的基准档也必须落在上限上，否则 `available_at_level` 无从解释。
        assert_eq!(caps.level, PermissionLevel::Read);
        assert!(tool_list_item(entry("read_file", true), &caps).available_at_level);
        assert!(!tool_list_item(entry("modify_file", true), &caps).available_at_level);
        assert!(!tool_list_item(entry("run_command", true), &caps).available_at_level);
    }
}
