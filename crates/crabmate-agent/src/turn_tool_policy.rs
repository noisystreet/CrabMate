//! 多角色工具白名单与并行批判定的纯逻辑（原 `agent_role_turn` 子集）。

use std::collections::HashSet;
use std::sync::Arc;

use crabmate_config::AgentConfig;
use crabmate_tools::registry_policy::tool_calls_allow_parallel_sync_batch;
use crabmate_tools::tool_dispatch::HandlerLookupTable;
use crabmate_types::ToolCall;

/// 本回合生效的角色 id：`request` 非空时优先，否则沿用 `persisted_active`。
pub fn effective_agent_role_id_for_turn(
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
) -> Option<String> {
    let req = request_agent_role.map(str::trim).filter(|s| !s.is_empty());
    if req.is_some() {
        return req.map(str::to_string);
    }
    persisted_active
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 与 `system_prompt_for_new_conversation` 对齐的**命名**角色 id（用于工具白名单）。
pub fn named_agent_role_for_tool_policy(
    cfg: &AgentConfig,
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
) -> Option<String> {
    if let Some(id) = effective_agent_role_id_for_turn(persisted_active, request_agent_role) {
        return Some(id);
    }
    cfg.roles_prompts
        .default_agent_role_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 执行层：当前回合允许的工具名；`None` 表示全量。
pub fn turn_allowed_tool_names_for_role(
    cfg: &AgentConfig,
    role_id: Option<&str>,
) -> Option<Arc<HashSet<String>>> {
    let id = role_id.map(str::trim).filter(|s| !s.is_empty())?;
    cfg.roles_prompts
        .agent_roles
        .get(id)
        .and_then(|spec| spec.allowed_tools.clone())
}

pub fn turn_allow_for_web_or_cli_job(
    cfg: &AgentConfig,
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
) -> Option<Arc<HashSet<String>>> {
    let id = named_agent_role_for_tool_policy(cfg, persisted_active, request_agent_role);
    turn_allowed_tool_names_for_role(cfg, id.as_deref())
}

/// 多角色工具白名单：`allow` 为 `None` 时不限制。
#[inline]
pub fn tool_allowed_for_turn(name: &str, allow: Option<&HashSet<String>>) -> bool {
    let Some(set) = allow else {
        return true;
    };
    if crabmate_tools::tool_naming::is_mcp_proxy_tool(name) {
        return set.contains("mcp");
    }
    set.contains(name)
}

pub fn turn_tool_denied_message(name: &str) -> String {
    format!("错误：当前 Agent 角色不允许调用工具 `{name}`（配置项 `allowed_tools`）。")
}

/// 在原有「只读并行批」判定之上，叠加多角色工具白名单。
pub fn tool_calls_allow_parallel_for_role(
    handler_lookup: &HandlerLookupTable,
    cfg: &AgentConfig,
    tool_calls: &[ToolCall],
    turn_allow: Option<&HashSet<String>>,
) -> bool {
    if !tool_calls_allow_parallel_sync_batch(handler_lookup, cfg, tool_calls) {
        return false;
    }
    if let Some(a) = turn_allow {
        tool_calls
            .iter()
            .all(|tc| tool_allowed_for_turn(tc.function.name.as_str(), Some(a)))
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_role_request_overrides_persisted() {
        assert_eq!(
            effective_agent_role_id_for_turn(Some("a"), Some("b")).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn turn_allow_blocks_unlisted_tool() {
        let mut allow = HashSet::new();
        allow.insert("read_file".to_string());
        assert!(!tool_allowed_for_turn("run_command", Some(&allow)));
        assert!(tool_allowed_for_turn("read_file", Some(&allow)));
    }
}
