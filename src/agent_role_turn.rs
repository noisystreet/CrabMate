//! Web/CLI 多角色工作台：按回合解析 `agent_role`、会话内切换时刷新首条 system、按角色裁剪工具列表并在执行层拒绝越权调用。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::context_bootstrap::prompt_compose::{
    FirstSystemComposeOpts, RoleSystemResolution, compose_first_system_for_turn,
    resolve_skills_base_dir,
};
use crate::types::{Message, ToolCall};

/// 本回合生效的角色 id：`request` 非空时优先，否则沿用 `persisted_active`（Web 会话存储 / REPL 内存）。
/// `None` 表示默认人格（`default_agent_role_id` 或全局 `system_prompt`），与历史未配置多角色时一致。
pub(crate) fn effective_agent_role_id_for_turn(
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

/// 与 `system_prompt_for_new_conversation` 对齐的**命名**角色 id（用于工具白名单）：请求 → 持久化 → 配置默认。
pub(crate) fn named_agent_role_for_tool_policy(
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

/// 回合结束后写入存储的 `active_agent_role`：本请求显式传了 `agent_role` 时用请求值，否则保持 `persisted_active`。
pub(crate) fn persisted_agent_role_after_turn(
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

fn last_user_message_text(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| m.role.trim().eq_ignore_ascii_case("user"))
        .map(|m| crate::types::message_content_into_text_lossy(m.content.clone()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 将首条 `system` 更新为新角色正文（保留后续 transcript）；含 L4 与可选 L5（skills top-k）。
pub(crate) fn apply_agent_role_switch_to_messages(
    cfg: &AgentConfig,
    messages: &mut [Message],
    role_id: Option<&str>,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    workspace_root: Option<&Path>,
    user_msg_for_skills: Option<&str>,
) -> Result<(), String> {
    let last_user_owned = last_user_message_text(messages);
    let skills_user = user_msg_for_skills
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(last_user_owned.as_deref());
    let skills_base = workspace_root.map(resolve_skills_base_dir);
    let sys = compose_first_system_for_turn(
        cfg,
        tool_recorder,
        FirstSystemComposeOpts {
            agent_role: role_id,
            user_msg_for_skills: skills_user,
            skills_base_dir: skills_base,
            role_resolution: RoleSystemResolution::Lenient,
        },
    )?;
    let mut found_system = false;
    for m in messages.iter_mut() {
        if m.role == "system" {
            m.content = Some(sys.into());
            m.name = None;
            found_system = true;
            break;
        }
    }
    if !found_system {
        return Err("会话缺少首条 system 消息，无法切换角色".to_string());
    }
    Ok(())
}

fn normalized_role_key(a: Option<&str>, b: Option<&str>) -> bool {
    match (
        a.map(str::trim).filter(|s| !s.is_empty()),
        b.map(str::trim).filter(|s| !s.is_empty()),
    ) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// 已有会话且请求中的 `agent_role` 与持久化不一致时，刷新首条 `system`。
pub(crate) fn maybe_apply_mid_session_agent_role_switch(
    cfg: &AgentConfig,
    messages: &mut [Message],
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
    tool_recorder: &Arc<crate::tool_stats::ToolOutcomeRecorder>,
    workspace_root: Option<&Path>,
    user_msg_for_skills: &str,
) -> Result<(), String> {
    if messages.is_empty() {
        return Ok(());
    }
    let req = request_agent_role.map(str::trim).filter(|s| !s.is_empty());
    let Some(req_id) = req else {
        return Ok(());
    };
    if normalized_role_key(Some(req_id), persisted_active) {
        return Ok(());
    }
    apply_agent_role_switch_to_messages(
        cfg,
        messages,
        Some(req_id),
        tool_recorder,
        workspace_root,
        Some(user_msg_for_skills),
    )
}

/// 按角色 `allowed_tools` 过滤 `tools`（`None` 表示不限制）。`mcp__` 前缀工具仅在允许集合显式包含 `"mcp"` 时保留。
pub(crate) fn filter_tools_for_agent_role(
    tools: &[crate::types::Tool],
    allowed: Option<&HashSet<String>>,
) -> Vec<crate::types::Tool> {
    let Some(set) = allowed else {
        return tools.to_vec();
    };
    let mcp_allowed = set.contains("mcp");
    tools
        .iter()
        .filter(|t| {
            let n = t.function.name.as_str();
            if n.starts_with("mcp__") {
                return mcp_allowed;
            }
            set.contains(n)
        })
        .cloned()
        .collect()
}

/// 执行层：当前回合允许的工具名（与送进模型的列表一致）；`None` 表示全量。
pub(crate) fn turn_allowed_tool_names_for_role(
    cfg: &AgentConfig,
    role_id: Option<&str>,
) -> Option<Arc<HashSet<String>>> {
    let id = role_id.map(str::trim).filter(|s| !s.is_empty())?;
    cfg.roles_prompts
        .agent_roles
        .get(id)
        .and_then(|spec| spec.allowed_tools.clone())
}

pub(crate) fn turn_allow_for_web_or_cli_job(
    cfg: &AgentConfig,
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
) -> Option<Arc<HashSet<String>>> {
    let id = named_agent_role_for_tool_policy(cfg, persisted_active, request_agent_role);
    turn_allowed_tool_names_for_role(cfg, id.as_deref())
}

/// 多角色工具白名单：`allow` 为 `None` 时不限制。
#[inline]
pub(crate) fn tool_allowed_for_turn(name: &str, allow: Option<&HashSet<String>>) -> bool {
    let Some(set) = allow else {
        return true;
    };
    if name.starts_with("mcp__") {
        return set.contains("mcp");
    }
    set.contains(name)
}

pub(crate) fn turn_tool_denied_message(name: &str) -> String {
    format!("错误：当前 Agent 角色不允许调用工具 `{name}`（配置项 `allowed_tools`）。")
}

/// 在原有「只读并行批」判定之上，叠加多角色工具白名单。
pub(crate) fn tool_calls_allow_parallel_for_role(
    handler_lookup: &crate::tool_registry::HandlerLookupTable,
    cfg: &AgentConfig,
    tool_calls: &[ToolCall],
    turn_allow: Option<&HashSet<String>>,
) -> bool {
    if !crate::tool_registry::tool_calls_allow_parallel_sync_batch(handler_lookup, cfg, tool_calls)
    {
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
        assert_eq!(
            effective_agent_role_id_for_turn(Some("a"), None).as_deref(),
            Some("a")
        );
        assert_eq!(effective_agent_role_id_for_turn(None, None), None);
    }

    #[test]
    fn filter_tools_respects_set_and_mcp_prefix() {
        use crate::types::{FunctionDef, Tool};
        let tools = vec![
            Tool {
                typ: "function".into(),
                function: FunctionDef {
                    name: "read_file".into(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
            Tool {
                typ: "function".into(),
                function: FunctionDef {
                    name: "mcp__x".into(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let mut s = HashSet::new();
        s.insert("read_file".to_string());
        let f = filter_tools_for_agent_role(&tools, Some(&s));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].function.name, "read_file");

        let mut s2 = HashSet::new();
        s2.insert("mcp".to_string());
        let f2 = filter_tools_for_agent_role(&tools, Some(&s2));
        assert_eq!(f2.len(), 1);
        assert_eq!(f2[0].function.name, "mcp__x");
    }
}
