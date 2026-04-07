//! Workspace 侧栏：拉取目录树与在切换到 Workspace 视图时自动刷新。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::WorkspaceData;
use crate::app_prefs::SidePanelView;
use crate::workspace_shell::reload_workspace_panel;

/// 返回与历史行为一致的 `reload_workspace_panel` 封装（供 SSE `on_workspace_changed`、侧栏等复用）。
pub(super) fn make_refresh_workspace(
    workspace_loading: RwSignal<bool>,
    workspace_err: RwSignal<Option<String>>,
    workspace_path_draft: RwSignal<String>,
    workspace_data: RwSignal<Option<WorkspaceData>>,
    workspace_subtree_expanded: RwSignal<HashSet<String>>,
    workspace_subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    workspace_subtree_loading: RwSignal<HashSet<String>>,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        spawn_local(async move {
            reload_workspace_panel(
                workspace_loading,
                workspace_err,
                workspace_path_draft,
                workspace_data,
                workspace_subtree_expanded,
                workspace_subtree_cache,
                workspace_subtree_loading,
            )
            .await;
        });
    })
}

/// 初始化完成且侧栏为 Workspace 时拉取一次。
pub(super) fn wire_workspace_refresh_when_visible(
    side_panel_view: RwSignal<SidePanelView>,
    initialized: RwSignal<bool>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_workspace = Arc::clone(&refresh_workspace);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::Workspace) && initialized.get() {
                refresh_workspace();
            }
        }
    });
}
