//! 壳布局常量、侧栏视图枚举与状态栏展示合并逻辑（持久化在 **`/user-data/prefs`**）。

use crate::api::StatusData;

/// 合法 `data-theme` 取值（与 **`frontend/themes/*.css`**、`index.html` 链接顺序一致）。
pub const THEME_SLUGS: &[&str] = &["dark", "light", "material", "high-contrast"];

#[must_use]
pub fn normalize_theme_slug(raw: &str) -> String {
    let t = raw.trim();
    if THEME_SLUGS.contains(&t) {
        t.to_string()
    } else {
        "light".to_string()
    }
}

pub const DEFAULT_SIDE_WIDTH: f64 = 280.0;
pub const MIN_SIDE_WIDTH: f64 = 200.0;
pub const MAX_SIDE_WIDTH: f64 = 560.0;
/// 为左侧对话列预留的最小宽度（视口过窄时仍允许侧栏拖到 `MIN_SIDE_WIDTH`，由 flex 挤压主列）。
pub const MIN_CHAT_RESERVE_PX: f64 = 240.0;
pub const AUTO_SCROLL_RESUME_GAP_PX: i32 = 24;
/// 流式跟读：距底 ≤ 该值视为 live edge，用 scrollHeight 增量追底（见 `scroll_anchor`）。
pub const STICKY_BOTTOM_THRESHOLD_PX: i32 = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidePanelView {
    None,
    Workspace,
    Tasks,
    /// GitHub Pull Requests / CI（在线模式侧栏）。
    PullRequests,
    /// 思维与工具调试台（与工作区共用右列宽度与 `side-pane` 布局）。
    DebugConsole,
}

/// 状态栏「模型」：本机保存的 `client_llm.model` 非空时优先，否则用 `/status`。
pub fn status_bar_effective_model(server: Option<&StatusData>, stored_model: &str) -> String {
    let t = stored_model.trim();
    if !t.is_empty() {
        t.to_string()
    } else {
        server
            .map(|d| d.model.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

/// 状态栏「base_url」：本机 `client_llm.api_base` 非空时优先，否则用 `/status`。
pub fn status_bar_effective_api_base(server: Option<&StatusData>, stored_api_base: &str) -> String {
    let t = stored_api_base.trim();
    if !t.is_empty() {
        t.to_string()
    } else {
        server
            .map(|d| d.api_base.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

/// 状态栏「上下文窗口 token 上限」：本机 `client_llm.llm_context_tokens` 非空且可解析为正数时优先，否则用 `/status`。
#[must_use]
pub fn status_bar_effective_llm_context_tokens(
    server: Option<&StatusData>,
    stored_llm_context_tokens: &str,
) -> u32 {
    let t = stored_llm_context_tokens.trim();
    if !t.is_empty() {
        if let Ok(n) = t.parse::<u32>() {
            if n > 0 {
                return n;
            }
        }
    }
    server.map(|d| d.llm_context_tokens).unwrap_or(0)
}

/// 新会话（尚无服务端 `conversation_id`）时状态栏用的 system-only prompt token 粗估。
#[must_use]
pub fn status_bar_new_session_baseline_prompt_tokens(
    server: Option<&StatusData>,
    selected_agent_role: Option<&str>,
) -> Option<u32> {
    let sd = server?;
    let map = &sd.tiktoken_new_session_baseline_by_agent_role;
    if map.is_empty() {
        return None;
    }
    if let Some(role) = selected_agent_role.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(&n) = map.get(role) {
            return Some(n);
        }
    }
    if let Some(role) = sd
        .default_agent_role_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(&n) = map.get(role) {
            return Some(n);
        }
    }
    map.get("").copied()
}

/// Web 状态栏「default」选项对应 `None`。
///
/// 服务端 `active_agent_role` 与配置 `default_agent_role_id` 相同时，语义上是默认档而非用户显式点选的下拉项。
#[must_use]
pub fn status_bar_selected_agent_role_from_persisted(
    persisted: Option<&str>,
    default_agent_role_id: Option<&str>,
) -> Option<String> {
    let p = persisted?.trim();
    if p.is_empty() {
        return None;
    }
    if default_agent_role_id.is_some_and(|d| d == p) {
        return None;
    }
    Some(p.to_string())
}

pub fn clamp_side_width_for_viewport(w: f64) -> f64 {
    let win = web_sys::window()
        .and_then(|win| win.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let max_w = (win - MIN_CHAT_RESERVE_PX).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
    w.clamp(MIN_SIDE_WIDTH, max_w)
}

#[cfg(test)]
mod theme_slug_tests {
    use super::normalize_theme_slug;

    #[test]
    fn unknown_theme_falls_back_to_light() {
        assert_eq!(normalize_theme_slug("nope"), "light");
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(normalize_theme_slug(" dark \n"), "dark");
    }

    #[test]
    fn material_accepted() {
        assert_eq!(normalize_theme_slug("material"), "material");
    }

    #[test]
    fn high_contrast_accepted() {
        assert_eq!(normalize_theme_slug("high-contrast"), "high-contrast");
    }
}

#[cfg(test)]
mod status_bar_agent_role_tests {
    use super::status_bar_selected_agent_role_from_persisted;

    #[test]
    fn default_role_id_maps_to_ui_none() {
        assert_eq!(
            status_bar_selected_agent_role_from_persisted(Some("main"), Some("main")),
            None
        );
    }

    #[test]
    fn explicit_named_role_preserved() {
        assert_eq!(
            status_bar_selected_agent_role_from_persisted(Some("coder"), Some("main")).as_deref(),
            Some("coder")
        );
    }
}
