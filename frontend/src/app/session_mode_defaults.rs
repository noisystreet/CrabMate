//! 从 `/status` 解析角色默认会话模式（与底栏换角、prefs 冷启动共用）。

use crate::api::StatusData;

/// 规范化 `ask` / `plan` / `act`；非法则 `None`。
#[must_use]
pub fn normalize_session_mode_slug(raw: &str) -> Option<String> {
    let m = raw.trim().to_ascii_lowercase();
    matches!(m.as_str(), "ask" | "plan" | "act").then_some(m)
}

/// 命名角色有配置则用之，否则回落全局 `default_session_mode`；清除角色时用全局默认。
#[must_use]
pub fn default_session_mode_for_agent_role(
    status: &StatusData,
    role_id: Option<&str>,
) -> Option<String> {
    let raw = match role_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => status
            .agent_role_default_session_modes
            .get(id)
            .map(String::as_str)
            .unwrap_or(status.default_session_mode.as_str()),
        None => status.default_session_mode.as_str(),
    };
    normalize_session_mode_slug(raw)
}
