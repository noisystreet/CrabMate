//! 壳级偏好与 **`document.documentElement`** 同步（持久化在 **`/user-data/prefs`**，见 [`crate::user_prefs_sync`]）。
//!
//! # 与其它模块分工
//!
//! - **主题 slug 白名单**：[`crate::app_prefs::THEME_SLUGS`] / [`normalize_theme_slug`]（加载偏好时在 [`crate::user_prefs_sync`] 中规范化）。
//! - **首屏壳 UI 快照**：[`read_shell_ui_initial_snapshot`] 聚合主题/语言/侧栏宽度等读路径，供 [`super::app_signals::ShellUISignals::new`] 单点消费。
//! - **会话 JSON**：[`crate::storage`] / [`crate::app::chat::session_storage`]。
//! - **`client_llm.*` / Bearer**：[`crate::api::client_llm_storage`]。
//!
//! 新增「首屏就读 / Effect 里写磁盘或改 DOM」的壳偏好时，优先在本模块加函数，避免在多个 `wire_*` 文件里散落 `set_item`。

use leptos::prelude::GetUntracked;
use wasm_bindgen::JsCast;

use crate::app::app_signals::AppSignals;
use crate::app_prefs::{DEFAULT_SIDE_WIDTH, SidePanelView};
use crate::i18n::Locale;
use crate::session_typography_prefs::{session_chat_font_stack_css, session_ui_font_stack_css};

/// 首屏默认（真实偏好由 [`crate::user_prefs_sync::wire_load_user_prefs_from_server`] 异步覆盖）。
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
        theme: "light".to_string(),
        bg_decor: true,
        locale: Locale::ZhHans,
        status_bar_visible: true,
        side_panel_view: SidePanelView::Workspace,
        side_width: DEFAULT_SIDE_WIDTH,
        editor_layout_mode: false,
        session_ui_font: "default".to_string(),
        session_chat_font: "default".to_string(),
    }
}

/// `GET /user-data/prefs` 灌入信号后，将主题/语言/字体等反映到 DOM。
pub(crate) fn apply_loaded_prefs_to_dom(app: &AppSignals) {
    persist_theme_to_storage_and_dom(&app.shell_ui.theme.get_untracked());
    apply_locale_html_lang(app.shell_ui.locale.get_untracked());
    persist_bg_decor_to_storage_and_dom(app.shell_ui.bg_decor.get_untracked());
    persist_session_typography_to_storage_and_dom(
        &app.shell_ui.session_ui_font.get_untracked(),
        &app.shell_ui.session_chat_font.get_untracked(),
    );
    apply_shell_layout_dom_flags(app.shell_ui.editor_layout_mode.get_untracked());
}

/// 会话模式界面 / 聊天列字体：在 `<html>` 上维护 `--crabmate-ui-font-family` / `--crabmate-chat-font-family`。
pub(crate) fn persist_session_typography_to_storage_and_dom(ui_slug: &str, chat_slug: &str) {
    let ui = crate::session_typography_prefs::normalize_session_ui_font(ui_slug);
    let chat = crate::session_typography_prefs::normalize_session_chat_font(chat_slug);
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

/// 设置 `data-theme`（持久化由 [`crate::user_prefs_sync`] 负责）。
pub(crate) fn persist_theme_to_storage_and_dom(theme: &str) {
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

/// 背景装饰：维护 `data-bg-decor`。
pub(crate) fn persist_bg_decor_to_storage_and_dom(bg_decor: bool) {
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
    None
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
