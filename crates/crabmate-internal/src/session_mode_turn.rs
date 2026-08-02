//! 会话模式（Ask / Plan / Act）解析与回合解析优先级。

use crabmate_config::AgentConfig;
use crabmate_types::{SessionMode, parse_optional_session_mode, parse_session_mode};

use crate::agent_role_turn::named_agent_role_for_tool_policy;

/// 解析请求 / 会话中的模式；空则回落到角色默认或配置默认。
///
/// 优先级：本轮请求 → 会话持久化 → 当前命名角色 `default_session_mode` → 配置默认。
pub fn resolve_session_mode_for_turn(
    request_mode: Option<&str>,
    persisted_mode: Option<&str>,
    role_default_mode: Option<SessionMode>,
    default_mode: SessionMode,
) -> Result<SessionMode, String> {
    if let Some(m) = parse_optional_session_mode(request_mode)? {
        return Ok(m);
    }
    if let Some(m) = parse_optional_session_mode(persisted_mode)? {
        return Ok(m);
    }
    if let Some(m) = role_default_mode {
        return Ok(m);
    }
    Ok(default_mode)
}

/// 按当前回合命名角色读取可选的角色默认 mode。
#[must_use]
pub fn role_default_session_mode_for_turn(
    cfg: &AgentConfig,
    persisted_active: Option<&str>,
    request_agent_role: Option<&str>,
) -> Option<SessionMode> {
    let id = named_agent_role_for_tool_policy(cfg, persisted_active, request_agent_role)?;
    cfg.roles_prompts
        .agent_roles
        .get(id.as_str())
        .and_then(|spec| spec.default_session_mode)
}

/// 按命名角色解析初始会话模式（无请求 / 无持久化时）：角色默认 → 全局默认。
#[must_use]
pub fn resolve_initial_session_mode(
    cfg: &AgentConfig,
    request_agent_role: Option<&str>,
) -> SessionMode {
    match resolve_session_mode_for_turn(
        None,
        None,
        role_default_session_mode_for_turn(cfg, None, request_agent_role),
        cfg.roles_prompts.default_session_mode,
    ) {
        Ok(m) => m,
        Err(_) => SessionMode::Act,
    }
}

/// 配置串 → [`SessionMode`]；非法时 warn 并回退 Act。
pub fn session_mode_from_config_str(raw: &str) -> SessionMode {
    match parse_session_mode(raw) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                error = %e,
                raw = %raw,
                "invalid default_session_mode; falling back to act"
            );
            SessionMode::Act
        }
    }
}

/// Ask / Plan 时返回 `true`（调用方应挂 ReviewReadonly）；Act 为 `false`。
#[must_use]
pub fn session_mode_requires_readonly_tools(mode: SessionMode) -> bool {
    mode.requires_readonly_tools()
}

/// 模式附录文件相对约定（与 `system_prompt_file` 相同解析规则）。
#[must_use]
pub fn session_mode_appendix_relpath(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Ask => "config/prompts/mode_ask.md",
        SessionMode::Plan => "config/prompts/mode_plan.md",
        SessionMode::Act => "config/prompts/mode_act.md",
    }
}

/// 回合结束后写入存储的 `active_session_mode`：本请求显式传了 `session_mode` 时用请求值，否则保持 `persisted`。
pub fn persisted_session_mode_after_turn(
    persisted_active: Option<&str>,
    request_session_mode: Option<&str>,
) -> Option<String> {
    let req = request_session_mode
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if req.is_some() {
        return req.map(str::to_string);
    }
    persisted_active
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_request_over_persisted() {
        let m = resolve_session_mode_for_turn(
            Some("ask"),
            Some("act"),
            Some(SessionMode::Plan),
            SessionMode::Act,
        )
        .unwrap();
        assert_eq!(m, SessionMode::Ask);
    }

    #[test]
    fn resolve_falls_back_to_persisted() {
        let m = resolve_session_mode_for_turn(
            None,
            Some("plan"),
            Some(SessionMode::Ask),
            SessionMode::Act,
        )
        .unwrap();
        assert_eq!(m, SessionMode::Plan);
    }

    #[test]
    fn resolve_falls_back_to_role_default() {
        let m = resolve_session_mode_for_turn(None, None, Some(SessionMode::Ask), SessionMode::Act)
            .unwrap();
        assert_eq!(m, SessionMode::Ask);
    }

    #[test]
    fn resolve_falls_back_to_default() {
        let m = resolve_session_mode_for_turn(None, None, None, SessionMode::Ask).unwrap();
        assert_eq!(m, SessionMode::Ask);
    }

    #[test]
    fn persist_prefers_request_when_present() {
        assert_eq!(
            persisted_session_mode_after_turn(Some("act"), Some("ask")).as_deref(),
            Some("ask")
        );
    }

    #[test]
    fn persist_keeps_previous_when_request_absent() {
        assert_eq!(
            persisted_session_mode_after_turn(Some("plan"), None).as_deref(),
            Some("plan")
        );
        assert_eq!(persisted_session_mode_after_turn(None, None), None);
    }

    /// 门控关闭会 clear 工具收窄；Ask 必须在其后仍能挂上只读（与 `run_dispatch` 顺序一致）。
    #[test]
    fn ask_mode_readonly_survives_gate_clear_ordering() {
        let mut step_constraint: Option<&'static str> = None;
        if session_mode_requires_readonly_tools(SessionMode::Ask) {
            step_constraint = Some("ReviewReadonly");
        }
        assert_eq!(step_constraint, Some("ReviewReadonly"));
    }
}
