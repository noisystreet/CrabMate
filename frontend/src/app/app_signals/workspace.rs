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
