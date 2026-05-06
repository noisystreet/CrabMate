//! 壳级偏好写入 **`localStorage`** 与 **`document.documentElement`** 的收口（主题、背景装饰、`<html lang>`、角色键）。
//!
//! # 与其它模块分工
//!
//! - **键名与通用读写**：[`crate::app_prefs`]（`THEME_KEY`、`store_bool_key`、侧栏视图等）；句柄经 [`super::local_storage_index`]。
//! - **会话 JSON**：[`crate::storage`] / [`super::app_shell_effects::session_storage`]。
//! - **`client_llm.*` / Bearer**：[`crate::api::client_llm_storage`]。
//!
//! 新增「首屏就读 / Effect 里写磁盘或改 DOM」的壳偏好时，优先在本模块加函数，避免在多个 `wire_*` 文件里散落 `set_item`。

use crate::app_prefs::{BG_DECOR_KEY, CM_ROLE_KEY, THEME_KEY, store_bool_key};
use crate::i18n::Locale;

use super::local_storage_index;

#[must_use]
pub(crate) fn read_theme_initial() -> String {
    local_storage_index::handle()
        .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
        .unwrap_or_else(|| "light".to_string())
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
