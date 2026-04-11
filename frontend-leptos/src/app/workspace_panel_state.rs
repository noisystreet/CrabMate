//! 工作区侧栏：目录树、根路径草稿、设置/浏览忙状态等 **RwSignal** 聚合。
//!
//! 与 [`super::chat_session_state::ChatSessionSignals`] 类似，减少 `App` → `side_column_view` / `make_refresh_workspace` 的参数传递。

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::api::WorkspaceData;

/// 右栏「工作区」面板相关的响应式句柄（不含任务清单；任务仍由 `App` 单独传）。
#[derive(Clone, Copy)]
pub struct WorkspacePanelSignals {
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
}
