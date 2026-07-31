//! 壳布局常量、侧栏视图枚举与状态栏展示合并逻辑（持久化在 **`/user-data/prefs`**）。

use std::sync::atomic::{AtomicI8, Ordering};

use crate::api::StatusData;

/// 偏好里可选的主题（含 **`system`**＝跟随 OS 明暗；解析到 CSS 见 [`resolve_data_theme_slug`]）。
pub const THEME_SLUGS: &[&str] = &["system", "dark", "light", "material", "high-contrast"];

/// 可写在 `<html data-theme>` 上的 CSS 预设（**不含** `system`）。
pub const THEME_CSS_SLUGS: &[&str] = &["dark", "light", "material", "high-contrast"];

pub const THEME_SYSTEM: &str = "system";

/// Tauri 桌面侧 `os_prefers_dark_theme` 提示：`-1` 未知，`0` 浅，`1` 深。
/// Linux WebKit 的 `matchMedia` 常忽略 GNOME `prefer-dark`，需以此覆盖。
static TAURI_OS_DARK_HINT: AtomicI8 = AtomicI8::new(-1);

/// 由桌面 invoke 结果写入；`None` 表示清除提示、回退 matchMedia。
pub fn set_tauri_os_prefers_dark_hint(dark: Option<bool>) {
    let v = match dark {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    };
    TAURI_OS_DARK_HINT.store(v, Ordering::Relaxed);
}

#[must_use]
pub fn normalize_theme_slug(raw: &str) -> String {
    let t = raw.trim();
    if THEME_SLUGS.contains(&t) {
        t.to_string()
    } else {
        "light".to_string()
    }
}

/// OS 是否偏好深色；无 `window` / `matchMedia` / Tauri 提示时返回 `None`。
#[must_use]
pub fn system_prefers_color_scheme_dark() -> Option<bool> {
    match TAURI_OS_DARK_HINT.load(Ordering::Relaxed) {
        0 => return Some(false),
        1 => return Some(true),
        _ => {}
    }
    let mql = match_media_list("(prefers-color-scheme: dark)")?;
    Some(media_query_list_matches(&mql))
}

fn match_media_list(query: &str) -> Option<js_sys::Object> {
    // 宿主 `cargo test` 非 wasm：js-sys Reflect 会 panic。
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = query;
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let window = web_sys::window()?;
        let f = js_sys::Reflect::get(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str("matchMedia"),
        )
        .ok()?;
        let f = f.dyn_into::<js_sys::Function>().ok()?;
        let v = f
            .call1(window.as_ref(), &wasm_bindgen::JsValue::from_str(query))
            .ok()?;
        if v.is_null() || v.is_undefined() {
            return None;
        }
        v.dyn_into::<js_sys::Object>().ok()
    }
}

fn media_query_list_matches(mql: &js_sys::Object) -> bool {
    js_sys::Reflect::get(mql.as_ref(), &wasm_bindgen::JsValue::from_str("matches"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 将偏好 slug 解析为 CSS `data-theme`（`system` → `dark` / `light`）。
#[must_use]
pub fn resolve_data_theme_slug(pref: &str) -> String {
    let pref = normalize_theme_slug(pref);
    let css = if pref == THEME_SYSTEM {
        match system_prefers_color_scheme_dark() {
            Some(true) => "dark".to_string(),
            Some(false) | None => "light".to_string(),
        }
    } else {
        pref
    };
    if THEME_CSS_SLUGS.contains(&css.as_str()) {
        css
    } else {
        "light".to_string()
    }
}

pub const DEFAULT_SIDE_WIDTH: f64 = 280.0;
pub const MIN_SIDE_WIDTH: f64 = 200.0;
pub const MAX_SIDE_WIDTH: f64 = 560.0;
/// 为左侧对话列预留的最小宽度（视口过窄时仍允许侧栏拖到 `MIN_SIDE_WIDTH`，由 flex 挤压主列）。
pub const MIN_CHAT_RESERVE_PX: f64 = 240.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidePanelView {
    None,
    Workspace,
    Tasks,
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
    use super::{THEME_CSS_SLUGS, THEME_SYSTEM, normalize_theme_slug, resolve_data_theme_slug};

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

    #[test]
    fn system_pref_accepted() {
        assert_eq!(normalize_theme_slug("system"), THEME_SYSTEM);
    }

    #[test]
    fn resolve_non_system_unchanged() {
        assert_eq!(resolve_data_theme_slug("material"), "material");
        assert!(THEME_CSS_SLUGS.contains(&"material"));
    }

    #[test]
    fn resolve_system_is_css_dark_or_light() {
        // 非 wasm 测到无 window → light；wasm 浏览器测可能为 dark/light。
        let resolved = resolve_data_theme_slug("system");
        assert!(resolved == "dark" || resolved == "light", "got {resolved}");
        assert!(!THEME_CSS_SLUGS.contains(&THEME_SYSTEM));
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
