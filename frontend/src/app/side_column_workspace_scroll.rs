//! 工作区侧栏滚动区：加载骨架与已加载内容（从 `side_column.rs` 拆出以降低组件圈复杂度）。
//! 工作区根路径已迁至顶栏正中（见 [`super::workspace_root_actions::ShellTopbarWorkspaceRoot`]）。

use leptos::prelude::*;
use std::sync::Arc;

use crate::i18n::{self, Locale};
use crate::workspace_context_menu::{
    WorkspaceContextMenuActions, WorkspaceContextMenuLayer, WorkspaceTreeChromeSignals,
};
use crate::workspace_tree::{WorkspaceFilesystemTree, WorkspaceFilesystemTreeInput};

use super::workspace_panel_state::WorkspacePanelSignals;

#[component]
fn WorkspaceSideCardScrollSkeleton(locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <div class="skeleton-stack" aria-busy="true" prop:aria-label=move || i18n::ws_loading_aria(locale.get())>
            <ul class="workspace-list workspace-list-skeleton">
                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
            </ul>
        </div>
    }
}

#[component]
fn WorkspaceSideCardLoaded(
    locale: RwSignal<Locale>,
    ws: WorkspacePanelSignals,
    insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    ctx_actions: StoredValue<WorkspaceContextMenuActions>,
) -> impl IntoView {
    view! {
        <div class="workspace-side-card-loaded">
            <Show when=move || {
                ws.workspace_err.get().is_some()
                    || ws.workspace_data.get().and_then(|d| d.error).is_some()
            }>
                <div class="msg-error">{move || {
                    ws.workspace_err
                        .get()
                        .or_else(|| ws.workspace_data.get().and_then(|d| d.error))
                        .unwrap_or_default()
                }}</div>
            </Show>
            <div
                class="workspace-tree-shell"
                on:contextmenu=move |ev: web_sys::MouseEvent| {
                    crate::workspace_tree::handle_workspace_tree_panel_context_menu(
                        ev,
                        ws.workspace_context_menu,
                    );
                }
            >
                <WorkspaceFilesystemTree input=WorkspaceFilesystemTreeInput {
                    workspace_data: ws.workspace_data,
                    subtree_expanded: ws.workspace_subtree_expanded,
                    subtree_cache: ws.workspace_subtree_cache,
                    subtree_loading: ws.workspace_subtree_loading,
                    chrome: WorkspaceTreeChromeSignals {
                        context_menu: ws.workspace_context_menu,
                        pending_create: ws.workspace_pending_create,
                    },
                    locale,
                    workspace_err: ws.workspace_err,
                    create_actions: ctx_actions,
                    on_file_double_click: insert_workspace_file_ref,
                    on_file_single_click,
                } />
                <WorkspaceContextMenuLayer
                    workspace_context_menu=ws.workspace_context_menu
                    workspace_pending_create=ws.workspace_pending_create
                    subtree_expanded=ws.workspace_subtree_expanded
                    subtree_cache=ws.workspace_subtree_cache
                    subtree_loading=ws.workspace_subtree_loading
                    locale=locale
                    workspace_err=ws.workspace_err
                    actions=ctx_actions.get_value()
                />
            </div>
        </div>
    }
}

/// 工作区侧栏已加载 / 骨架内容（对话侧栏与 IDE 左栏共用）。
#[component]
pub(crate) fn WorkspaceSideCardScrollInner(
    locale: RwSignal<Locale>,
    ws: WorkspacePanelSignals,
    insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    ctx_actions: StoredValue<WorkspaceContextMenuActions>,
) -> impl IntoView {
    view! {
        {move || {
            if ws.workspace_loading.get() {
                view! { <WorkspaceSideCardScrollSkeleton locale=locale /> }.into_any()
            } else {
                view! {
                    <WorkspaceSideCardLoaded
                        locale=locale
                        ws=ws
                        insert_workspace_file_ref=insert_workspace_file_ref
                        on_file_single_click=on_file_single_click
                        ctx_actions=ctx_actions
                    />
                }
                .into_any()
            }
        }}
    }
}
