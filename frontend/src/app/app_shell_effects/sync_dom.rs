//! 主题 / 语言 / 背景装饰：同步 `localStorage` 与 `document.documentElement`。
//!
//! 具体读写收口见 [`crate::app::shell_prefs_storage`]。

use leptos::prelude::*;

use crate::app::shell_prefs_storage;
use crate::i18n::Locale;

pub fn wire_sync_theme_to_storage_and_dom(theme: RwSignal<String>) {
    Effect::new(move |_| {
        shell_prefs_storage::persist_theme_to_storage_and_dom(&theme.get());
    });
}

pub fn wire_sync_locale_html_lang(locale: RwSignal<Locale>) {
    Effect::new(move |_| {
        shell_prefs_storage::apply_locale_html_lang(locale.get());
    });
}

pub fn wire_sync_bg_decor_to_storage_and_dom(bg_decor: RwSignal<bool>) {
    Effect::new(move |_| {
        shell_prefs_storage::persist_bg_decor_to_storage_and_dom(bg_decor.get());
    });
}

pub fn wire_sync_session_typography_to_storage_and_dom(
    session_ui_font: RwSignal<String>,
    session_chat_font: RwSignal<String>,
) {
    Effect::new(move |_| {
        shell_prefs_storage::persist_session_typography_to_storage_and_dom(
            &session_ui_font.get(),
            &session_chat_font.get(),
        );
    });
}

/// `<html>` 布局标记；Tauri 下始终无边框（Web 顶栏不受影响）。
pub fn wire_sync_tauri_shell_dom(editor_layout_mode: RwSignal<bool>) {
    Effect::new(move |_| {
        shell_prefs_storage::apply_shell_layout_dom_flags(editor_layout_mode.get());
    });
    Effect::new(|_| {
        crate::tauri_shell::tauri_apply_frameless_window_chrome();
    });
}
