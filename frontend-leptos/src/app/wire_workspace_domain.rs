//! 工作区与变更集：changelog 拉取、模态 `innerHTML`、侧栏可见时刷新目录树（从 `app/mod.rs` 迁入，阶段 B）。

use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;

use crate::app_prefs::SidePanelView;
use crate::session_sync::SessionSyncState;

use super::changelist_modal::{wire_changelist_body_inner_html, wire_changelist_fetch_effects};
use super::workspace_panel::wire_workspace_refresh_when_visible;

/// 注册变更集 fetch / DOM 绑定与工作区可见刷新（缩短 [`wire_workspace_domain_effects`] 形参列表）。
#[derive(Clone)]
pub(crate) struct WireWorkspaceDomainEffectsArgs {
    pub session_sync: RwSignal<SessionSyncState>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub changelist_modal_loading: RwSignal<bool>,
    pub changelist_modal_err: RwSignal<Option<String>>,
    pub changelist_modal_html: RwSignal<String>,
    pub changelist_modal_rev: RwSignal<u64>,
    pub markdown_render: RwSignal<bool>,
    pub changelist_body_ref: NodeRef<Div>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub initialized: RwSignal<bool>,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
}

/// 注册变更集 fetch / DOM 绑定与工作区可见刷新。
pub(crate) fn wire_workspace_domain_effects(args: WireWorkspaceDomainEffectsArgs) {
    let WireWorkspaceDomainEffectsArgs {
        session_sync,
        changelist_fetch_nonce,
        changelist_modal_loading,
        changelist_modal_err,
        changelist_modal_html,
        changelist_modal_rev,
        markdown_render,
        changelist_body_ref,
        side_panel_view,
        initialized,
        refresh_workspace,
    } = args;
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
