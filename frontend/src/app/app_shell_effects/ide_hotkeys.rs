//! IDE 布局全局快捷键：`Ctrl/Cmd+S` 保存、`Ctrl/Cmd+Shift+S` 全部保存。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;

use crate::app::app_signals::ShellUISignals;

#[derive(Clone, Copy)]
pub struct IdeEditorHotkeySignals {
    pub shell_ui: ShellUISignals,
    pub ide_settings_page: RwSignal<bool>,
}

/// 在编辑器布局可见时注册保存快捷键（文本框内同样生效）。
pub fn wire_ide_editor_hotkeys(signals: IdeEditorHotkeySignals) {
    let IdeEditorHotkeySignals {
        shell_ui,
        ide_settings_page,
    } = signals;
    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if !shell_ui.editor_layout_mode.get_untracked() {
                return;
            }
            if ide_settings_page.get_untracked() {
                return;
            }
            if !(ev.meta_key() || ev.ctrl_key()) {
                return;
            }
            if !ev.key().eq_ignore_ascii_case("s") {
                return;
            }
            ev.prevent_default();
            if ev.shift_key() {
                shell_ui
                    .ide_save_all_nonce
                    .update(|n| *n = n.saturating_add(1));
            } else {
                shell_ui
                    .ide_save_active_nonce
                    .update(|n| *n = n.saturating_add(1));
            }
        });
        on_cleanup(move || h.remove());
    });
}
