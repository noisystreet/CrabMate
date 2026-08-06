//! 工作区树、路径草稿与设置/浏览忙状态。

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::api::WorkspaceData;

#[derive(Clone, Copy)]
pub struct WorkspaceSignals {
    pub workspace_data: RwSignal<Option<WorkspaceData>>,
    pub workspace_subtree_expanded: RwSignal<HashSet<String>>,
    pub workspace_subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    pub workspace_subtree_loading: RwSignal<HashSet<String>>,
    pub workspace_err: RwSignal<Option<String>>,
    pub workspace_loading: RwSignal<bool>,
    pub workspace_path_draft: RwSignal<String>,
    pub workspace_set_err: RwSignal<Option<String>>,
    pub workspace_set_busy: RwSignal<bool>,
    pub workspace_pick_busy: RwSignal<bool>,
    /// 项目池模式：选择/新建项目弹窗。
    pub workspace_project_modal_open: RwSignal<bool>,
    /// 项目池：Clone 远程仓库弹窗。
    pub workspace_clone_modal_open: RwSignal<bool>,
    /// `GET /workspace/projects` 的 enabled（Clone 菜单可见性）。
    pub workspace_pool_enabled: RwSignal<bool>,
    /// 浏览器无项目池时：最近列表 + 路径输入弹窗（替代 `window.prompt`）。
    pub workspace_browser_pick_modal_open: RwSignal<bool>,
    /// 最近打开的工作区根（新在前；来自 `prefs.recent_workspace_roots`）。
    pub recent_workspace_roots: RwSignal<Vec<String>>,
    /// 首启 `GET /user-data/prefs` 已结束（成功或失败）；为 false 时勿 PUT prefs。
    pub user_prefs_hydrated: RwSignal<bool>,
    pub workspace_context_menu:
        RwSignal<Option<crate::workspace_context_menu::WorkspaceContextAnchor>>,
    pub workspace_pending_create:
        RwSignal<Option<crate::workspace_context_menu::WorkspacePendingCreate>>,
}

impl WorkspaceSignals {
    pub fn new() -> Self {
        Self {
            workspace_data: RwSignal::new(None),
            workspace_subtree_expanded: RwSignal::new(HashSet::new()),
            workspace_subtree_cache: RwSignal::new(HashMap::new()),
            workspace_subtree_loading: RwSignal::new(HashSet::new()),
            workspace_err: RwSignal::new(None),
            workspace_loading: RwSignal::new(false),
            workspace_path_draft: RwSignal::new(String::new()),
            workspace_set_err: RwSignal::new(None),
            workspace_set_busy: RwSignal::new(false),
            workspace_pick_busy: RwSignal::new(false),
            workspace_project_modal_open: RwSignal::new(false),
            workspace_clone_modal_open: RwSignal::new(false),
            workspace_pool_enabled: RwSignal::new(false),
            workspace_browser_pick_modal_open: RwSignal::new(false),
            recent_workspace_roots: RwSignal::new(Vec::new()),
            user_prefs_hydrated: RwSignal::new(false),
            workspace_context_menu: RwSignal::new(None),
            workspace_pending_create: RwSignal::new(None),
        }
    }
}

impl Default for WorkspaceSignals {
    fn default() -> Self {
        Self::new()
    }
}
