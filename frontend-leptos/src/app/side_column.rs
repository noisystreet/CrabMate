//! 右列：拖拽分隔条、视图工具栏、工作区与任务侧栏。

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use web_sys::KeyboardEvent;

use crate::api::{TaskItem, TasksData, WorkspaceData, fetch_workspace_pick, post_workspace_set};
use crate::app_prefs::SidePanelView;
use crate::i18n::{self, Locale};
use crate::workspace_shell::{begin_side_column_resize, reload_workspace_panel};
use crate::workspace_tree::WorkspaceFilesystemTree;

#[component]
fn SideColumnTasksLoadedPane(
    tasks_err: RwSignal<Option<String>>,
    tasks_data: RwSignal<TasksData>,
    toggle_task: Arc<dyn Fn(String) + Send + Sync>,
) -> impl IntoView {
    view! {
        <div class="side-card-loaded">
            <Show when=move || tasks_err.get().is_some()>
                <div class="msg-error">{move || tasks_err.get().unwrap_or_default()}</div>
            </Show>
            <ul class=move || {
                if tasks_data.get().items.is_empty() {
                    "tasks-list"
                } else {
                    "tasks-list list-stagger"
                }
            }>
                {move || {
                    tasks_data
                        .get()
                        .items
                        .into_iter()
                        .enumerate()
                        .map(|(i, t): (usize, TaskItem)| {
                            let toggle_task = Arc::clone(&toggle_task);
                            let id = t.id.clone();
                            let done = t.done;
                            let stagger = i.to_string();
                            view! {
                                <li style=format!("--list-stagger: {stagger}")>
                                    <input
                                        type="checkbox"
                                        prop:checked=done
                                        on:change=move |_| toggle_task(id.clone())
                                    />
                                    <span>{t.title}</span>
                                </li>
                            }
                        })
                        .collect_view()
                }}
            </ul>
        </div>
    }
}

#[component]
fn SideColumnTasksCard(
    locale: RwSignal<Locale>,
    tasks_loading: RwSignal<bool>,
    tasks_err: RwSignal<Option<String>>,
    tasks_data: RwSignal<TasksData>,
    refresh_tasks: Arc<dyn Fn() + Send + Sync>,
    toggle_task: Arc<dyn Fn(String) + Send + Sync>,
) -> impl IntoView {
    view! {
        <div class="side-pane" style:flex="1" style:min-width="0">
            <div class="side-card">
                <div class="side-card-head">
                    <div class="side-head-main">
                        <div class="side-pane-title">{move || i18n::tasks_title(locale.get())}</div>
                        <span class="side-head-stat">{move || {
                            if tasks_loading.get() {
                                i18n::tasks_loading(locale.get()).to_string()
                            } else if tasks_err.get().is_some() {
                                i18n::tasks_error(locale.get()).to_string()
                            } else {
                                let items = tasks_data.get().items;
                                let total = items.len();
                                let done = items.iter().filter(|t| t.done).count();
                                i18n::tasks_done_ratio(locale.get(), done, total)
                            }
                        }}</span>
                    </div>
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm side-head-action"
                        on:click={
                            let refresh_tasks = Arc::clone(&refresh_tasks);
                            move |_| refresh_tasks()
                        }
                    >
                        {move || i18n::tasks_refresh(locale.get())}
                    </button>
                </div>
                <div class="side-card-body">
                    <Show when=move || tasks_loading.get()>
                        <div class="skeleton-stack" aria-busy="true" prop:aria-label=move || i18n::tasks_loading_aria(locale.get())>
                            <ul class="tasks-list tasks-list-skeleton">
                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                            </ul>
                        </div>
                    </Show>
                    <Show when=move || !tasks_loading.get()>
                        <SideColumnTasksLoadedPane
                            tasks_err=tasks_err
                            tasks_data=tasks_data
                            toggle_task=toggle_task.clone()
                        />
                    </Show>
                </div>
            </div>
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
pub fn side_column_view(
    locale: RwSignal<Locale>,
    side_resize_dragging: RwSignal<bool>,
    side_panel_view: RwSignal<SidePanelView>,
    side_width: RwSignal<f64>,
    side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    side_resize_handles: Rc<
        RefCell<
            Option<(
                leptos_dom::helpers::WindowListenerHandle,
                leptos_dom::helpers::WindowListenerHandle,
            )>,
        >,
    >,
    view_menu_open: RwSignal<bool>,
    status_bar_visible: RwSignal<bool>,
    settings_modal: RwSignal<bool>,
    workspace_data: RwSignal<Option<WorkspaceData>>,
    workspace_subtree_expanded: RwSignal<HashSet<String>>,
    workspace_subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    workspace_subtree_loading: RwSignal<HashSet<String>>,
    workspace_err: RwSignal<Option<String>>,
    workspace_loading: RwSignal<bool>,
    workspace_path_draft: RwSignal<String>,
    workspace_set_err: RwSignal<Option<String>>,
    workspace_set_busy: RwSignal<bool>,
    workspace_pick_busy: RwSignal<bool>,
    tasks_data: RwSignal<TasksData>,
    tasks_err: RwSignal<Option<String>>,
    tasks_loading: RwSignal<bool>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    refresh_tasks: Arc<dyn Fn() + Send + Sync>,
    toggle_task: Arc<dyn Fn(String) + Send + Sync>,
    changelist_modal_open: RwSignal<bool>,
    changelist_fetch_nonce: RwSignal<u64>,
) -> impl IntoView {
    view! {
                <div
                    class="column-resize-handle"
                    class:column-resize-handle-off=move || {
                        matches!(side_panel_view.get(), SidePanelView::None)
                    }
                    role="separator"
                    aria-orientation="vertical"
                    prop:aria-label=move || i18n::side_resize_handle(locale.get())
                    on:mousedown={
                        let sess = Rc::clone(&side_resize_session);
                        let hands = Rc::clone(&side_resize_handles);
                        move |ev| {
                            begin_side_column_resize(
                                ev,
                                side_panel_view,
                                side_width,
                                side_resize_dragging,
                                Rc::clone(&sess),
                                Rc::clone(&hands),
                            );
                        }
                    }
                ></div>

                <div
                    class:side-column-resizing=move || side_resize_dragging.get()
                    class=move || {
                        let mut c = String::from("side-column");
                        if matches!(side_panel_view.get(), SidePanelView::None) {
                            c.push_str(" side-column-rail-only");
                        }
                        c
                    }
                    style:width=move || {
                        if matches!(side_panel_view.get(), SidePanelView::None) {
                            "0px".to_string()
                        } else {
                            format!("{}px", side_width.get())
                        }
                    }
                >
                        <div class="shell-main-toolbar" role="toolbar" prop:aria-label=move || i18n::side_toolbar_aria(locale.get())>
                            <div class="toolbar-view-wrap">
                                <Show when=move || view_menu_open.get()>
                                    <div
                                        class="toolbar-view-backdrop"
                                        on:click=move |_| view_menu_open.set(false)
                                    ></div>
                                </Show>
                                <button
                                    type="button"
                                    class="btn btn-secondary btn-sm toolbar-view-trigger"
                                    data-testid="side-view-trigger"
                                    class:active=move || !matches!(side_panel_view.get(), SidePanelView::None)
                                    class:toolbar-view-trigger-open=move || view_menu_open.get()
                                    on:click=move |_| view_menu_open.update(|o| *o = !*o)
                                    prop:title=move || i18n::side_view_menu_title(locale.get())
                                >
                                    {move || i18n::side_view_label(locale.get(), view_menu_open.get())}
                                </button>
                                <Show when=move || view_menu_open.get()>
                                    <div class="toolbar-view-menu" role="menu" prop:aria-label=move || i18n::side_view_menu_aria(locale.get())>
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::None)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::None);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            {move || i18n::side_panel_hide(locale.get())}
                                        </button>
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            data-testid="side-panel-workspace-menu"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::Workspace)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::Workspace);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            {move || i18n::side_panel_workspace(locale.get())}
                                        </button>
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::Tasks)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::Tasks);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            {move || i18n::side_panel_tasks(locale.get())}
                                        </button>
                                    </div>
                                </Show>
                            </div>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                class:active=move || status_bar_visible.get()
                                on:click=move |_| status_bar_visible.update(|v| *v = !*v)
                                prop:title=move || i18n::side_status_btn_title(locale.get())
                            >
                                {move || i18n::side_status_btn(locale.get())}
                            </button>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                on:click=move |_| settings_modal.set(true)
                                prop:title=move || i18n::side_settings_title(locale.get())
                            >
                                {move || i18n::side_settings_btn(locale.get())}
                            </button>
                        </div>
                        <div class="side-body">
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::Workspace)>
                                <div class="side-pane" style:flex="1" style:min-width="0">
                                    <div class="side-card">
                                        <Show when=move || {
                                            workspace_loading.get()
                                                || workspace_err.get().is_some()
                                                || workspace_data
                                                    .get()
                                                    .and_then(|d| d.error.clone())
                                                    .is_some()
                                        }>
                                            <div class="side-card-head">
                                                <div class="side-head-main">
                                                    <span class="side-head-stat">{move || {
                                                        if workspace_loading.get() {
                                                            i18n::changelist_loading(locale.get()).to_string()
                                                        } else {
                                                            i18n::tasks_error(locale.get()).to_string()
                                                        }
                                                    }}</span>
                                                </div>
                                            </div>
                                        </Show>
                                        <div class="side-card-body workspace-side-card-body" data-testid="workspace-panel">
                                            <div class="workspace-side-card-scroll">
                                            {move || {
                                                if workspace_loading.get() {
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
                                                    .into_any()
                                                } else {
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
                                                                        prop:value=move || workspace_path_draft.get()
                                                                        on:input=move |ev| {
                                                                            workspace_path_draft
                                                                                .set(event_target_value(&ev));
                                                                        }
                                                                        on:keydown=move |ev: KeyboardEvent| {
                                                                            if ev.key() != "Enter" {
                                                                                return;
                                                                            }
                                                                            ev.prevent_default();
                                                                            workspace_set_err.set(None);
                                                                            let p = workspace_path_draft
                                                                                .get()
                                                                                .trim()
                                                                                .to_string();
                                                                            if p.is_empty() {
                                                                                workspace_set_err.set(Some(
                                                                                    i18n::ws_path_required(locale.get()).to_string(),
                                                                                ));
                                                                                return;
                                                                            }
                                                                            if workspace_set_busy.get()
                                                                                || workspace_pick_busy.get()
                                                                                || workspace_loading.get()
                                                                            {
                                                                                return;
                                                                            }
                                                                            workspace_set_busy.set(true);
                                                                            let loc = locale.get_untracked();
                                                                            spawn_local(async move {
                                                                                match post_workspace_set(Some(p), loc).await {
                                                                                    Ok(_) => {
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
                                                                                    }
                                                                                    Err(e) => {
                                                                                        workspace_set_err.set(Some(e));
                                                                                    }
                                                                                }
                                                                                workspace_set_busy.set(false);
                                                                            });
                                                                        }
                                                                    />
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-secondary btn-sm workspace-set-browse"
                                                                        prop:title=move || i18n::ws_browse_title(locale.get())
                                                                        prop:disabled=move || {
                                                                            workspace_pick_busy.get()
                                                                                || workspace_set_busy.get()
                                                                                || workspace_loading.get()
                                                                        }
                                                                        on:click=move |_| {
                                                                            workspace_set_err.set(None);
                                                                            workspace_pick_busy.set(true);
                                                                            let loc_pick = locale.get_untracked();
                                                                            spawn_local(async move {
                                                                                match fetch_workspace_pick().await {
                                                                                    Ok(Some(p)) => {
                                                                                        workspace_path_draft.set(p.clone());
                                                                                        workspace_set_err.set(None);
                                                                                        match post_workspace_set(Some(p), loc_pick).await {
                                                                                            Ok(_) => {
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
                                                                                            }
                                                                                            Err(e) => {
                                                                                                workspace_set_err.set(Some(e));
                                                                                            }
                                                                                        }
                                                                                    }
                                                                                    Ok(None) => {
                                                                                        workspace_set_err.set(Some(
                                                                                            i18n::ws_pick_none(locale.get()).to_string(),
                                                                                        ));
                                                                                    }
                                                                                    Err(e) => {
                                                                                        workspace_set_err.set(Some(e));
                                                                                    }
                                                                                }
                                                                                workspace_pick_busy.set(false);
                                                                            });
                                                                        }
                                                                    >
                                                                        {move || {
                                                                            if workspace_pick_busy.get() {
                                                                                i18n::ws_browse_busy(locale.get()).to_string()
                                                                            } else {
                                                                                i18n::ws_browse_label(locale.get()).to_string()
                                                                            }
                                                                        }}
                                                                    </button>
                                                                </div>
                                                                <Show when=move || workspace_set_err.get().is_some()>
                                                                    <div class="msg-error workspace-set-error">{move || {
                                                                        workspace_set_err
                                                                            .get()
                                                                            .unwrap_or_default()
                                                                    }}</div>
                                                                </Show>
                                                            </div>
                                                            <Show when=move || {
                                                                workspace_err.get().is_some()
                                                                    || workspace_data.get().and_then(|d| d.error).is_some()
                                                            }>
                                                                <div class="msg-error">{move || {
                                                                    workspace_err
                                                                        .get()
                                                                        .or_else(|| workspace_data.get().and_then(|d| d.error))
                                                                        .unwrap_or_default()
                                                                }}</div>
                                                            </Show>
                                                            <WorkspaceFilesystemTree
                                                                workspace_data=workspace_data
                                                                subtree_expanded=workspace_subtree_expanded
                                                                subtree_cache=workspace_subtree_cache
                                                                subtree_loading=workspace_subtree_loading
                                                                locale=locale
                                                            />
                                                        </>
                                                    }
                                                    .into_any()
                                                }
                                            }}
                                            </div>
                                            <div class="workspace-list-refresh workspace-list-refresh-row">
                                                <button
                                                    type="button"
                                                    class="btn btn-secondary btn-sm workspace-list-refresh-btn"
                                                    on:click={
                                                        let refresh_workspace = Arc::clone(&refresh_workspace);
                                                        move |_| refresh_workspace()
                                                    }
                                                >
                                                    {move || i18n::ws_refresh_list(locale.get())}
                                                </button>
                                                <button
                                                    type="button"
                                                    class="btn btn-muted btn-sm workspace-changelog-btn"
                                                    prop:title=move || i18n::ws_changelog_title(locale.get())
                                                    on:click=move |_| {
                                                        changelist_modal_open.set(true);
                                                        changelist_fetch_nonce
                                                            .update(|x| *x = x.wrapping_add(1));
                                                    }
                                                >
                                                    {move || i18n::ws_changelog_btn(locale.get())}
                                                </button>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </Show>
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::Tasks)>
                                <SideColumnTasksCard
                                    locale=locale
                                    tasks_loading=tasks_loading
                                    tasks_err=tasks_err
                                    tasks_data=tasks_data
                                    refresh_tasks=refresh_tasks.clone()
                                    toggle_task=toggle_task.clone()
                                />
                            </Show>
                        </div>
                </div>
    }
}
