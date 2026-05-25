//! IDE 布局与侧栏联动的壳级 `Effect`（偏好持久化见 [`crate::user_prefs_sync`]）。

use leptos::prelude::*;

/// 进入 IDE（编辑器）主区布局时自动收起桌面端左侧会话栏。
pub fn wire_collapse_sidebar_rail_when_ide_layout(
    editor_layout_mode: RwSignal<bool>,
    sidebar_rail_collapsed: RwSignal<bool>,
) {
    Effect::new(move |_| {
        if editor_layout_mode.get() {
            sidebar_rail_collapsed.set(true);
        }
    });
}

/// 进入 IDE 布局时关闭窄屏侧栏抽屉与聊天查找条（不隐藏 Web 顶栏）。
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
