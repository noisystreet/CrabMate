//! 壳级偏好写入 `localStorage`（侧栏视图、状态栏、角色、宽度等）。

use leptos::prelude::*;

use crate::app::shell_prefs_storage;
use crate::app_prefs::{
    EDITOR_LAYOUT_MODE_KEY, SIDEBAR_RAIL_COLLAPSED_KEY, STATUS_BAR_VISIBLE_KEY, SidePanelView,
    TASKS_VISIBLE_KEY, WORKSPACE_VISIBLE_KEY, WORKSPACE_WIDTH_KEY, store_bool_key, store_f64_key,
    store_side_panel_view,
};

pub fn wire_persist_side_panel_view_flags(side_panel_view: RwSignal<SidePanelView>) {
    Effect::new(move |_| {
        let v = side_panel_view.get();
        store_side_panel_view(v);
        store_bool_key(WORKSPACE_VISIBLE_KEY, matches!(v, SidePanelView::Workspace));
        store_bool_key(TASKS_VISIBLE_KEY, matches!(v, SidePanelView::Tasks));
    });
}

pub fn wire_persist_status_bar_visible(status_bar_visible: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(STATUS_BAR_VISIBLE_KEY, status_bar_visible.get());
    });
}

pub fn wire_persist_agent_role(selected_agent_role: RwSignal<Option<String>>) {
    Effect::new(move |_| {
        shell_prefs_storage::persist_agent_role_trimmed(selected_agent_role.get().as_deref());
    });
}

pub fn wire_persist_side_width(side_width: RwSignal<f64>) {
    Effect::new(move |_| {
        store_f64_key(WORKSPACE_WIDTH_KEY, side_width.get());
    });
}

/// 桌面端左侧会话栏收起状态写入 `localStorage`。
pub fn wire_persist_sidebar_rail_collapsed(sidebar_rail_collapsed: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(SIDEBAR_RAIL_COLLAPSED_KEY, sidebar_rail_collapsed.get());
    });
}

pub fn wire_persist_editor_layout_mode(editor_layout_mode: RwSignal<bool>) {
    Effect::new(move |_| {
        store_bool_key(EDITOR_LAYOUT_MODE_KEY, editor_layout_mode.get());
    });
}

/// 进入 IDE（编辑器）主区布局时自动收起桌面端左侧会话栏。
pub fn wire_persist_ide_editor_prefs(editor: crate::app::app_signals::IdeEditorSignals) {
    Effect::new(move |_| {
        editor.font_slug.get();
        editor.font_size_px.get();
        editor.line_numbers.get();
        editor.word_wrap.get();
        editor.tab_size.get();
        editor.persist_from_signals();
    });
}

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
