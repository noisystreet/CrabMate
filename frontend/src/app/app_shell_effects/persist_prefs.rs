//! IDE 布局与侧栏偏好联动（持久化见 [`crate::user_prefs_sync`]）。

use leptos::prelude::*;

use crate::app::ide_layout_switch::{IdeLayoutToggleSignals, remember_sidebar_before_ide};

/// 进入 IDE 时记住对话侧栏展开/收起；退出时由 [`crate::app::ide_layout_switch::exit_editor_layout`] 同步恢复。
pub fn wire_sidebar_rail_when_ide_layout(toggle: IdeLayoutToggleSignals) {
    Effect::new(move |_| {
        if toggle.editor_layout_mode.get() {
            remember_sidebar_before_ide(toggle);
        }
    });
}

pub fn wire_close_shell_chrome_when_ide_layout(
    editor_layout_mode: RwSignal<bool>,
    mobile_nav_open: RwSignal<bool>,
    chat_find_panel_open: RwSignal<bool>,
) {
    Effect::new(move |_| {
        if !editor_layout_mode.get() {
            return;
        }
        mobile_nav_open.set(false);
        chat_find_panel_open.set(false);
    });
}
