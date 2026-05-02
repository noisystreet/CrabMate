//! 工作区侧栏滚动区：加载骨架与已加载内容（从 `side_column.rs` 拆出以降低组件圈复杂度）。

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use std::sync::Arc;
use web_sys::KeyboardEvent;

use crate::api::{fetch_workspace_pick, post_workspace_set};
use crate::i18n::{self, Locale};
use crate::workspace_shell::reload_workspace_panel;
use crate::workspace_tree::WorkspaceFilesystemTree;

use super::workspace_panel_state::WorkspacePanelSignals;

#[component]
fn WorkspaceSideCardScrollSkeleton(locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <div class="skeleton-stack" aria-busy="true" prop:aria-label=move || i18n::ws_loading_aria(locale.get())>
            <div class="skeleton skeleton-block skeleton-ws-path"></div>
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
) -> impl IntoView {
    view! {
        <>
            <div class="workspace-set">
                <div class="workspace-set-label">{move || i18n::ws_root_label(locale.get())}</div>
                <div class="workspace-set-input-row">
                    <input
                        type="text"
                        class="workspace-set-input"
                        data-testid="workspace-root-input"
                        prop:placeholder=move || i18n::ws_input_ph(locale.get())
                        prop:title=move || i18n::ws_input_title(locale.get())
                        prop:value=move || ws.workspace_path_draft.get()
                        on:input=move |ev| {
                            ws.workspace_path_draft
                                .set(event_target_value(&ev));
                        }
                        on:keydown=move |ev: KeyboardEvent| {
                            if ev.key() != "Enter" {
                                return;
                            }
                            ev.prevent_default();
                            ws.workspace_set_err.set(None);
                            let p = ws.workspace_path_draft
                                .get()
                                .trim()
                                .to_string();
                            if p.is_empty() {
                                ws.workspace_set_err.set(Some(
                                    i18n::ws_path_required(locale.get()).to_string(),
                                ));
                                return;
                            }
                            if ws.workspace_set_busy.get()
                                || ws.workspace_pick_busy.get()
                                || ws.workspace_loading.get()
                            {
                                return;
                            }
                            ws.workspace_set_busy.set(true);
                            let loc = locale.get_untracked();
                            spawn_local(async move {
                                match post_workspace_set(Some(p), loc).await {
                                    Ok(_) => {
                                        reload_workspace_panel(
                                            ws.workspace_loading,
                                            ws.workspace_err,
                                            ws.workspace_path_draft,
                                            ws.workspace_data,
                                            ws.workspace_subtree_expanded,
                                            ws.workspace_subtree_cache,
                                            ws.workspace_subtree_loading,
                                            loc,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        ws.workspace_set_err.set(Some(e));
                                    }
                                }
                                ws.workspace_set_busy.set(false);
                            });
                        }
                    />
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm workspace-set-browse"
                        prop:title=move || i18n::ws_browse_title(locale.get())
                        prop:disabled=move || {
                            ws.workspace_pick_busy.get()
                                || ws.workspace_set_busy.get()
                                || ws.workspace_loading.get()
                        }
                        on:click=move |_| {
                            ws.workspace_set_err.set(None);
                            ws.workspace_pick_busy.set(true);
                            let loc_pick = locale.get_untracked();
                            spawn_local(async move {
                                match fetch_workspace_pick(loc_pick).await {
                                    Ok(Some(p)) => {
                                        ws.workspace_path_draft.set(p.clone());
                                        ws.workspace_set_err.set(None);
                                        match post_workspace_set(Some(p), loc_pick).await {
                                            Ok(_) => {
                                                reload_workspace_panel(
                                                    ws.workspace_loading,
                                                    ws.workspace_err,
                                                    ws.workspace_path_draft,
                                                    ws.workspace_data,
                                                    ws.workspace_subtree_expanded,
                                                    ws.workspace_subtree_cache,
                                                    ws.workspace_subtree_loading,
                                                    loc_pick,
                                                )
                                                .await;
                                            }
                                            Err(e) => {
                                                ws.workspace_set_err.set(Some(e));
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        ws.workspace_set_err.set(Some(
                                            i18n::ws_pick_none(locale.get()).to_string(),
                                        ));
                                    }
                                    Err(e) => {
                                        ws.workspace_set_err.set(Some(e));
                                    }
                                }
                                ws.workspace_pick_busy.set(false);
                            });
                        }
                    >
                        {move || {
                            if ws.workspace_pick_busy.get() {
                                i18n::ws_browse_busy(locale.get()).to_string()
                            } else {
                                i18n::ws_browse_label(locale.get()).to_string()
                            }
                        }}
                    </button>
                </div>
                <Show when=move || ws.workspace_set_err.get().is_some()>
                    <div class="msg-error workspace-set-error">{move || {
                        ws.workspace_set_err
                            .get()
                            .unwrap_or_default()
                    }}</div>
                </Show>
            </div>
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
            <WorkspaceFilesystemTree
                workspace_data=ws.workspace_data
                subtree_expanded=ws.workspace_subtree_expanded
                subtree_cache=ws.workspace_subtree_cache
                subtree_loading=ws.workspace_subtree_loading
                locale=locale
                on_file_double_click=insert_workspace_file_ref
            />
        </>
    }
}

#[component]
pub(super) fn WorkspaceSideCardScrollInner(
    locale: RwSignal<Locale>,
    ws: WorkspacePanelSignals,
    insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
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
                    />
                }
                .into_any()
            }
        }}
    }
}
