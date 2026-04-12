//! 右列：拖拽分隔条、视图工具栏、工作区与任务侧栏。

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use std::rc::Rc;
use std::sync::Arc;
use web_sys::KeyboardEvent;

use crate::api::{TaskItem, TasksData, fetch_workspace_pick, post_workspace_set};
use crate::app_prefs::SidePanelView;
use crate::i18n::{self, Locale};
use crate::sse_dispatch::ThinkingTraceInfo;
use crate::workspace_shell::{begin_side_column_resize, reload_workspace_panel};
use crate::workspace_tree::WorkspaceFilesystemTree;

use super::app_shell_ctx::AppShellCtx;

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

#[component]
fn SideColumnDebugConsoleCard(
    locale: RwSignal<Locale>,
    thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
) -> impl IntoView {
    view! {
        <div class="side-pane" style:flex="1" style:min-width="0">
            <div
                class="side-card"
                role="region"
                prop:aria-label=move || i18n::debug_console_region_aria(locale.get())
            >
                <div class="side-card-head">
                    <div class="side-head-main">
                        <div class="side-pane-title">{move || i18n::side_debug_console_btn(locale.get())}</div>
                    </div>
                </div>
                <div class="side-card-body debug-console-body">
                    {move || {
                        let rows = thinking_trace_log.get();
                        if rows.is_empty() {
                            let hint = i18n::debug_console_empty_hint(locale.get());
                            view! { <p class="debug-console-empty">{hint}</p> }.into_any()
                        } else {
                            rows.into_iter()
                                .enumerate()
                                .map(|(i, e)| {
                                    let summary = {
                                        let mut s = format!(
                                            "{} {}",
                                            e.op,
                                            e.title.as_deref().unwrap_or("")
                                        );
                                        if let Some(ref nid) = e.node_id {
                                            s.push_str(" · ");
                                            s.push_str(nid);
                                        }
                                        if let Some(ref pid) = e.parent_id {
                                            s.push_str(" ← ");
                                            s.push_str(pid);
                                        }
                                        s
                                    };
                                    let chunk = e.chunk.clone().unwrap_or_default();
                                    let snap = e.context_snapshot.clone().unwrap_or_default();
                                    view! {
                                        <details class="debug-trace-item">
                                            <summary class="debug-trace-summary">{i + 1} " · " {summary}</summary>
                                            <div class="debug-trace-block">
                                                <div class="debug-trace-label">"chunk"</div>
                                                <pre class="debug-trace-pre">{chunk}</pre>
                                            </div>
                                            <div class="debug-trace-block">
                                                <div class="debug-trace-label">"context"</div>
                                                <pre class="debug-trace-pre">{snap}</pre>
                                            </div>
                                        </details>
                                    }
                                    .into_any()
                                })
                                .collect_view()
                                .into_any()
                        }
                    }}
                </div>
            </div>
        </div>
    }
}

pub fn side_column_view(ctx: AppShellCtx) -> impl IntoView {
    let AppShellCtx {
        locale,
        side_resize_dragging,
        side_panel_view,
        side_width,
        side_resize_session,
        side_resize_handles,
        view_menu_open,
        status_bar_visible,
        settings_modal,
        workspace_panel: ws,
        status_tasks,
        refresh_workspace,
        refresh_tasks,
        toggle_task,
        changelist_modal_open,
        changelist_fetch_nonce,
        insert_workspace_file_ref,
        thinking_trace_log,
        ..
    } = ctx;
    let tasks_data = status_tasks.tasks_data;
    let tasks_err = status_tasks.tasks_err;
    let tasks_loading = status_tasks.tasks_loading;
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
                                    class="btn btn-secondary btn-sm toolbar-view-trigger shell-toolbar-icon-btn"
                                    data-testid="side-view-trigger"
                                    class:active=move || !matches!(side_panel_view.get(), SidePanelView::None)
                                    class:toolbar-view-trigger-open=move || view_menu_open.get()
                                    on:click=move |_| view_menu_open.update(|o| *o = !*o)
                                    prop:title=move || i18n::side_view_menu_title(locale.get())
                                    prop:aria-label=move || i18n::side_view_menu_aria(locale.get())
                                >
                                    <span class="toolbar-view-trigger-inner" aria-hidden="true">
                                        <svg
                                            class="shell-toolbar-icon"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                        >
                                            <rect x="3" y="3" width="7" height="18" rx="1" ry="1" />
                                            <rect x="14" y="3" width="7" height="18" rx="1" ry="1" />
                                        </svg>
                                        <svg
                                            class="toolbar-view-chevron"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                        >
                                            <polyline points="6 9 12 15 18 9" />
                                        </svg>
                                    </span>
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
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::DebugConsole)
                                            role="menuitem"
                                            prop:title=move || i18n::side_debug_console_title(locale.get())
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::DebugConsole);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            {move || i18n::side_debug_console_btn(locale.get())}
                                        </button>
                                    </div>
                                </Show>
                            </div>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm shell-toolbar-icon-btn"
                                class:active=move || status_bar_visible.get()
                                on:click=move |_| status_bar_visible.update(|v| *v = !*v)
                                prop:title=move || i18n::side_status_btn_title(locale.get())
                                prop:aria-label=move || i18n::side_status_btn_title(locale.get())
                            >
                                <svg
                                    class="shell-toolbar-icon"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    aria-hidden="true"
                                >
                                    <path d="M22 12h-4l-3 9L9 3l-3 9H2" />
                                </svg>
                            </button>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm shell-toolbar-icon-btn"
                                on:click=move |_| settings_modal.set(true)
                                prop:title=move || i18n::side_settings_title(locale.get())
                                prop:aria-label=move || i18n::side_settings_title(locale.get())
                            >
                                <svg
                                    class="shell-toolbar-icon"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    aria-hidden="true"
                                >
                                    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
                                    <circle cx="12" cy="12" r="3" />
                                </svg>
                            </button>
                        </div>
                        <div class="side-body">
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::Workspace)>
                                <div class="side-pane" style:flex="1" style:min-width="0">
                                    <div class="side-card">
                                        <Show when=move || {
                                            ws.workspace_loading.get()
                                                || ws.workspace_err.get().is_some()
                                                || ws.workspace_data
                                                    .get()
                                                    .and_then(|d| d.error.clone())
                                                    .is_some()
                                        }>
                                            <div class="side-card-head">
                                                <div class="side-head-main">
                                                    <span class="side-head-stat">{move || {
                                                        if ws.workspace_loading.get() {
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
                                                if ws.workspace_loading.get() {
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
                                                                                match fetch_workspace_pick().await {
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
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::DebugConsole)>
                                <SideColumnDebugConsoleCard locale=locale thinking_trace_log=thinking_trace_log />
                            </Show>
                        </div>
                </div>
    }
}
