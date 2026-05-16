//! 壳级偏好写入 **`localStorage`** 与 **`document.documentElement`** 的收口（主题、背景装饰、`<html lang>`、角色键）。
//!
//! # 与其它模块分工
//!
//! - **键名与通用读写**：[`crate::app_prefs`]（`THEME_KEY`、`THEME_SLUGS`、`normalize_theme_slug`、`store_bool_key`、侧栏视图等）；句柄经 [`super::local_storage_index`]。
//! - **首屏壳 UI 快照**：[`read_shell_ui_initial_snapshot`] 聚合主题/语言/侧栏宽度等读路径，供 [`super::app_signals::ShellUISignals::new`] 单点消费。
//! - **会话 JSON**：[`crate::storage`] / [`crate::app::chat::session_storage`]。
//! - **`client_llm.*` / Bearer**：[`crate::api::client_llm_storage`]。
//!
//! 新增「首屏就读 / Effect 里写磁盘或改 DOM」的壳偏好时，优先在本模块加函数，避免在多个 `wire_*` 文件里散落 `set_item`。

use wasm_bindgen::JsCast;

use crate::app_prefs::{
    BG_DECOR_KEY, CM_ROLE_KEY, DEFAULT_SIDE_WIDTH, EDITOR_LAYOUT_MODE_KEY, SESSION_CHAT_FONT_KEY,
    SESSION_UI_FONT_KEY, STATUS_BAR_VISIBLE_KEY, SidePanelView, THEME_KEY, WORKSPACE_WIDTH_KEY,
    load_bool_key, load_f64_key, load_side_panel_view, normalize_theme_slug, store_bool_key,
};
use crate::i18n::Locale;
use crate::session_typography_prefs::{
    read_session_chat_font_initial, read_session_ui_font_initial, session_chat_font_stack_css,
    session_ui_font_stack_css,
};

use super::local_storage_index;

#[must_use]
pub(crate) fn read_theme_initial() -> String {
    let raw = local_storage_index::handle()
        .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
        .unwrap_or_else(|| "light".to_string());
    normalize_theme_slug(&raw)
}

/// 首屏 [`super::app_signals::ShellUISignals`] 所需的 **`localStorage`** 域快照（单入口读键）。
#[derive(Clone)]
pub(crate) struct ShellUiInitialSnapshot {
    pub theme: String,
    pub bg_decor: bool,
    pub locale: Locale,
    pub status_bar_visible: bool,
    pub side_panel_view: SidePanelView,
    pub side_width: f64,
    pub editor_layout_mode: bool,
    pub session_ui_font: String,
    pub session_chat_font: String,
}

#[must_use]
pub(crate) fn read_shell_ui_initial_snapshot() -> ShellUiInitialSnapshot {
    ShellUiInitialSnapshot {
        theme: read_theme_initial(),
        bg_decor: load_bool_key(BG_DECOR_KEY, true),
        locale: crate::i18n::load_locale_from_storage(),
        status_bar_visible: load_bool_key(STATUS_BAR_VISIBLE_KEY, true),
        side_panel_view: load_side_panel_view(),
        side_width: load_f64_key(WORKSPACE_WIDTH_KEY, DEFAULT_SIDE_WIDTH),
        editor_layout_mode: load_bool_key(EDITOR_LAYOUT_MODE_KEY, false),
        session_ui_font: read_session_ui_font_initial(),
        session_chat_font: read_session_chat_font_initial(),
    }
}

/// 会话模式界面 / 聊天列字体：写入本机并在 `<html>` 上维护 `--crabmate-ui-font-family` / `--crabmate-chat-font-family`。
pub(crate) fn persist_session_typography_to_storage_and_dom(ui_slug: &str, chat_slug: &str) {
    let ui = crate::session_typography_prefs::normalize_session_ui_font(ui_slug);
    let chat = crate::session_typography_prefs::normalize_session_chat_font(chat_slug);
    if let Some(st) = local_storage_index::handle() {
        let _ = st.set_item(SESSION_UI_FONT_KEY, &ui);
        let _ = st.set_item(SESSION_CHAT_FONT_KEY, &chat);
    }
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(root) = doc.document_element() else {
        return;
    };
    let Some(html_root) = root.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let style = html_root.style();
    match session_ui_font_stack_css(&ui) {
        Some(stack) => {
            let _ = style.set_property("--crabmate-ui-font-family", stack);
        }
        None => {
            let _ = style.remove_property("--crabmate-ui-font-family");
        }
    }
    match session_chat_font_stack_css(&chat) {
        Some(stack) => {
            let _ = style.set_property("--crabmate-chat-font-family", stack);
        }
        None => {
            let _ = style.remove_property("--crabmate-chat-font-family");
        }
    }
}

/// 将主题写入本机并设置 `data-theme`（与 [`super::app_shell_effects::sync_dom::wire_sync_theme_to_storage_and_dom`] 语义一致）。
pub(crate) fn persist_theme_to_storage_and_dom(theme: &str) {
    if let Some(st) = local_storage_index::handle() {
        let _ = st.set_item(THEME_KEY, theme);
    }
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(root) = doc.document_element()
    {
        let _ = root.set_attribute("data-theme", theme);
    }
}

/// 将界面语言反映到 `<html lang>`（不写 `localStorage`；语言持久化在 i18n 路径）。
pub(crate) fn apply_locale_html_lang(locale: Locale) {
    let lang = locale.html_lang();
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let _ = doc
            .document_element()
            .map(|root| root.set_attribute("lang", lang));
    }
}

/// 背景装饰：布尔写入 `localStorage` 并维护 `data-bg-decor`。
pub(crate) fn persist_bg_decor_to_storage_and_dom(bg_decor: bool) {
    store_bool_key(BG_DECOR_KEY, bg_decor);
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(root) = doc.document_element()
    {
        if bg_decor {
            let _ = root.remove_attribute("data-bg-decor");
        } else {
            let _ = root.set_attribute("data-bg-decor", "plain");
        }
    }
}

#[must_use]
pub(crate) fn read_agent_role_initial() -> Option<String> {
    local_storage_index::handle()
        .and_then(|s| s.get_item(CM_ROLE_KEY).ok().flatten())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Tauri 壳标记与 IDE 布局标记（`data-tauri-shell` / `data-ide-layout`：窗口装饰与菜单栏拖拽，不隐藏 Web 顶栏）。
pub(crate) fn apply_shell_layout_dom_flags(editor_layout_mode: bool) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(root) = doc.document_element() else {
        return;
    };
    if crate::tauri_shell::tauri_shell_available() {
        let _ = root.set_attribute("data-tauri-shell", "");
    } else {
        let _ = root.remove_attribute("data-tauri-shell");
    }
    if editor_layout_mode {
        let _ = root.set_attribute("data-ide-layout", "");
    } else {
        let _ = root.remove_attribute("data-ide-layout");
    }
}

/// 经纪人角色：非空则 `set_item`，否则 `remove_item`。
pub(crate) fn persist_agent_role_trimmed(selected: Option<&str>) {
    let Some(st) = local_storage_index::handle() else {
        return;
    };
    match selected.map(str::trim).filter(|s| !s.is_empty()) {
        Some(role) => {
            let _ = st.set_item(CM_ROLE_KEY, role);
        }
        None => {
            let _ = st.remove_item(CM_ROLE_KEY);
        }
    }
}
