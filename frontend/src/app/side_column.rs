//! 右列：拖拽分隔条、视图工具栏、工作区与任务侧栏。

use crate::api::{TaskItem, TasksData};
use crate::app_prefs::SidePanelView;
use crate::i18n::{self, Locale};
use crate::sse_dispatch::ThinkingTraceInfo;
use leptos::prelude::*;
use std::sync::Arc;

use super::app_shell_ctx::SideColumnViewSignals;
use super::side_column_toolbar::{SideColumnResizeAndShellToolbar, SideColumnResizeToolbarSignals};
use super::side_column_workspace_scroll::WorkspaceSideCardScrollInner;
use super::workspace_panel_state::WorkspacePanelSignals;

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

#[component]
fn SideColumnWorkspaceCard(
    locale: RwSignal<Locale>,
    ws: WorkspacePanelSignals,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    changelist_modal_open: RwSignal<bool>,
    changelist_fetch_nonce: RwSignal<u64>,
    insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    view! {
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
                        <WorkspaceSideCardScrollInner
                            locale=locale
                            ws=ws
                            insert_workspace_file_ref=insert_workspace_file_ref
                        />
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
    }
}

pub fn side_column_view(signals: SideColumnViewSignals) -> impl IntoView {
    let SideColumnViewSignals {
        locale,
        side_resize_dragging,
        side_panel_view,
        side_width,
        side_resize_session,
        side_resize_handles,
        view_menu_open,
        status_bar_visible,
        settings_page,
        workspace_panel: ws,
        status_tasks,
        refresh_workspace,
        refresh_tasks,
        toggle_task,
        changelist_modal_open,
        changelist_fetch_nonce,
        insert_workspace_file_ref,
        thinking_trace_log,
    } = signals;
    let tasks_data = status_tasks.tasks_data;
    let tasks_err = status_tasks.tasks_err;
    let tasks_loading = status_tasks.tasks_loading;
    let resize_toolbar = SideColumnResizeToolbarSignals {
        locale,
        side_resize_dragging,
        side_panel_view,
        side_width,
        side_resize_session,
        side_resize_handles,
        view_menu_open,
        status_bar_visible,
        settings_page,
    };
    view! {
        <SideColumnResizeAndShellToolbar toolbar=resize_toolbar>
            <div class="side-body">
                <Show when=move || matches!(side_panel_view.get(), SidePanelView::Workspace)>
                    <SideColumnWorkspaceCard
                        locale=locale
                        ws=ws
                        refresh_workspace=refresh_workspace.clone()
                        changelist_modal_open=changelist_modal_open
                        changelist_fetch_nonce=changelist_fetch_nonce
                        insert_workspace_file_ref=insert_workspace_file_ref
                    />
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
        </SideColumnResizeAndShellToolbar>
    }
}
