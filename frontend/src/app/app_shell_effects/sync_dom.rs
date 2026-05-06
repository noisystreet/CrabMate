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
