//! 主题 / 语言 / 背景装饰：同步 `localStorage` 与 `document.documentElement`。
//!
//! 具体读写收口见 [`crate::app::shell_prefs_storage`]。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;

use crate::app::shell_prefs_storage;
use crate::app_prefs::THEME_SYSTEM;
use crate::i18n::Locale;
use crate::tauri_shell::{tauri_fetch_os_prefers_dark_hint, tauri_shell_available};

/// 已提交主题 + 设置壳开关：OS `prefers-color-scheme` 变化时勿冲掉外观草稿预览。
pub struct WireSyncThemeSignals {
    pub theme: RwSignal<String>,
    pub settings_modal: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
}

pub fn wire_sync_theme_to_storage_and_dom(sig: WireSyncThemeSignals) {
    let theme = sig.theme;
    let settings_modal = sig.settings_modal;
    let settings_page = sig.settings_page;
    Effect::new(move |_| {
        shell_prefs_storage::persist_theme_to_storage_and_dom(&theme.get());
    });
    // Linux 桌面：WebKit matchMedia 常不可靠，用 Tauri gsettings 提示覆盖后再刷一次 DOM。
    if tauri_shell_available() {
        spawn_local(async move {
            if tauri_fetch_os_prefers_dark_hint().await.is_some()
                && theme.get_untracked() == THEME_SYSTEM
                && !settings_modal.get_untracked()
                && !settings_page.get_untracked()
            {
                shell_prefs_storage::persist_theme_to_storage_and_dom(THEME_SYSTEM);
            }
        });
    }
    // 一次性监听 OS 明暗；仅当偏好为 `system` 且设置未打开时重刷 `data-theme`。
    // 设置打开时由外观草稿 Effect 独占 DOM，避免预览被 OS 变化冲掉。
    Effect::new(move |_| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(f) = js_sys::Reflect::get(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str("matchMedia"),
        ) else {
            return;
        };
        let Ok(f) = f.dyn_into::<js_sys::Function>() else {
            return;
        };
        let Ok(mql_v) = f.call1(
            window.as_ref(),
            &wasm_bindgen::JsValue::from_str("(prefers-color-scheme: dark)"),
        ) else {
            return;
        };
        if mql_v.is_null() || mql_v.is_undefined() {
            return;
        }
        let Ok(mql) = mql_v.dyn_into::<web_sys::EventTarget>() else {
            return;
        };
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if settings_modal.get_untracked() || settings_page.get_untracked() {
                return;
            }
            if theme.get_untracked() == THEME_SYSTEM {
                // 桌面：OS 变化后同步刷新 gsettings 提示（matchMedia 可能仍滞后）。
                if tauri_shell_available() {
                    spawn_local(async move {
                        let _ = tauri_fetch_os_prefers_dark_hint().await;
                        if theme.get_untracked() == THEME_SYSTEM
                            && !settings_modal.get_untracked()
                            && !settings_page.get_untracked()
                        {
                            shell_prefs_storage::persist_theme_to_storage_and_dom(THEME_SYSTEM);
                        }
                    });
                } else {
                    shell_prefs_storage::persist_theme_to_storage_and_dom(THEME_SYSTEM);
                }
            }
        });
        let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
        // Effect 无信号依赖，仅挂载一次；泄漏监听器与页面同寿。
        cb.forget();
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
