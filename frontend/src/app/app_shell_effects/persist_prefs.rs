//! 壳级偏好写入 `localStorage`（侧栏视图、状态栏、角色、宽度等）。

use leptos::prelude::*;

use crate::app_prefs::{
    CM_ROLE_KEY, SIDEBAR_RAIL_COLLAPSED_KEY, STATUS_BAR_VISIBLE_KEY, SidePanelView,
    TASKS_VISIBLE_KEY, WORKSPACE_VISIBLE_KEY, WORKSPACE_WIDTH_KEY, local_storage, store_bool_key,
    store_f64_key, store_side_panel_view,
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
        if let Some(st) = local_storage() {
            match selected_agent_role
                .get()
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(role) => {
                    let _ = st.set_item(CM_ROLE_KEY, role);
                }
                None => {
                    let _ = st.remove_item(CM_ROLE_KEY);
                }
            }
        }
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
