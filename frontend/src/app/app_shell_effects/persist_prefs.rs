//! IDE 布局与侧栏偏好联动（持久化见 [`crate::user_prefs_sync`]）。

use leptos::prelude::*;

/// 进入 IDE 时仅记住对话侧栏展开/收起，**不**改写信号（侧栏由 CSS 隐藏，避免动画与误持久化 `collapsed=true`）。
/// 退出 IDE 时恢复进入前的状态，避免首次回到对话时会话栏仍被折叠。
pub fn wire_sidebar_rail_when_ide_layout(
    editor_layout_mode: RwSignal<bool>,
    sidebar_rail_collapsed: RwSignal<bool>,
) {
    let collapsed_before_ide = RwSignal::new(None::<bool>);
    Effect::new(move |_| {
        if editor_layout_mode.get() {
            if collapsed_before_ide.get_untracked().is_none() {
                collapsed_before_ide.set(Some(sidebar_rail_collapsed.get_untracked()));
            }
            return;
        }
        if let Some(was) = collapsed_before_ide.get_untracked() {
            sidebar_rail_collapsed.set(was);
            collapsed_before_ide.set(None);
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
