//! 工作区与变更集：changelog 拉取、模态 `innerHTML`、侧栏可见时刷新目录树（从 `app/mod.rs` 迁入，阶段 B）。

use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;

use crate::app_prefs::SidePanelView;
use crate::session_sync::SessionSyncState;

use super::changelist_modal::{wire_changelist_body_inner_html, wire_changelist_fetch_effects};
use super::workspace_panel::wire_workspace_refresh_when_visible;

/// 注册变更集 fetch / DOM 绑定与工作区可见刷新。
#[allow(clippy::too_many_arguments)]
pub(crate) fn wire_workspace_domain_effects(
    session_sync: RwSignal<SessionSyncState>,
    changelist_fetch_nonce: RwSignal<u64>,
    changelist_modal_loading: RwSignal<bool>,
    changelist_modal_err: RwSignal<Option<String>>,
    changelist_modal_html: RwSignal<String>,
    changelist_modal_rev: RwSignal<u64>,
    markdown_render: RwSignal<bool>,
    changelist_body_ref: NodeRef<Div>,
    side_panel_view: RwSignal<SidePanelView>,
    initialized: RwSignal<bool>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
) {
    wire_changelist_fetch_effects(
        session_sync,
        changelist_fetch_nonce,
        changelist_modal_loading,
        changelist_modal_err,
        changelist_modal_html,
        changelist_modal_rev,
        markdown_render,
    );
    wire_changelist_body_inner_html(changelist_modal_html, changelist_body_ref);

    wire_workspace_refresh_when_visible(
        side_panel_view,
        initialized,
        Arc::clone(&refresh_workspace),
    );
}
