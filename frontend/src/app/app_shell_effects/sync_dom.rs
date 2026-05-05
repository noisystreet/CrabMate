//! 主题 / 语言 / 背景装饰：同步 `localStorage` 与 `document.documentElement`。

use leptos::prelude::*;

use crate::app_prefs::{BG_DECOR_KEY, THEME_KEY, local_storage, store_bool_key};
use crate::i18n::Locale;

pub fn wire_sync_theme_to_storage_and_dom(theme: RwSignal<String>) {
    Effect::new(move |_| {
        let t = theme.get();
        if let Some(st) = local_storage() {
            let _ = st.set_item(THEME_KEY, &t);
        }
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            let _ = root.set_attribute("data-theme", &t);
        }
    });
}

pub fn wire_sync_locale_html_lang(locale: RwSignal<Locale>) {
    Effect::new(move |_| {
        let lang = locale.get().html_lang();
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc
                .document_element()
                .map(|root| root.set_attribute("lang", lang));
        }
    });
}

pub fn wire_sync_bg_decor_to_storage_and_dom(bg_decor: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(BG_DECOR_KEY, bg_decor.get());
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            if bg_decor.get() {
                let _ = root.remove_attribute("data-bg-decor");
            } else {
                let _ = root.set_attribute("data-bg-decor", "plain");
            }
        }
    });
}
