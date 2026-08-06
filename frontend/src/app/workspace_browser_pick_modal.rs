//! 浏览器（非 Tauri / 无项目池）：最近工作区列表 + 绝对路径输入，替代 `window.prompt`。

use gloo_timers::future::TimeoutFuture;
use leptos::html::{Div, Input};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::app::workspace_root_actions::{
    WorkspaceRootPickHandle, commit_workspace_root, workspace_inputs_blocked,
};
use crate::i18n;
use crate::user_data_bootstrap::workspace_recent_menu_label;

#[derive(Clone, Copy)]
pub struct WorkspaceBrowserPickModalSignals {
    pub open: RwSignal<bool>,
    pub workspace_pick: WorkspaceRootPickHandle,
}

fn submit_path(open: RwSignal<bool>, pick: WorkspaceRootPickHandle, path_draft: RwSignal<String>) {
    if workspace_inputs_blocked(pick.ws) {
        return;
    }
    let loc = pick.locale.get_untracked();
    let p = path_draft.get_untracked().trim().to_string();
    if p.is_empty() {
        pick.ws
            .workspace_set_err
            .set(Some(i18n::ws_path_required(loc).to_string()));
        return;
    }
    pick.ws.workspace_set_err.set(None);
    pick.ws.workspace_path_draft.set(p.clone());
    pick.ws.workspace_set_busy.set(true);
    spawn_local(async move {
        if commit_workspace_root(pick.chat, pick.ws, p, loc).await {
            open.set(false);
        }
    });
}

fn open_recent_path(open: RwSignal<bool>, pick: WorkspaceRootPickHandle, path: String) {
    if workspace_inputs_blocked(pick.ws) {
        return;
    }
    let p = path.trim().to_string();
    if p.is_empty() {
        return;
    }
    pick.ws.workspace_set_err.set(None);
    pick.ws.workspace_path_draft.set(p.clone());
    pick.ws.workspace_set_busy.set(true);
    let loc = pick.locale.get_untracked();
    spawn_local(async move {
        if commit_workspace_root(pick.chat, pick.ws, p, loc).await {
            open.set(false);
        }
    });
}

#[component]
fn WorkspaceBrowserPickRecentList(
    locale: RwSignal<crate::i18n::Locale>,
    recent: RwSignal<Vec<String>>,
    pick: WorkspaceRootPickHandle,
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="workspace-browser-pick-section">
            <h3 class="workspace-browser-pick-heading">
                {move || i18n::ws_browser_pick_recent_heading(locale.get())}
            </h3>
            <Show
                when=move || !recent.get().is_empty()
                fallback=move || {
                    view! {
                        <p class="modal-hint" data-testid="workspace-browser-pick-recent-empty">
                            {move || i18n::ws_browser_pick_recent_empty(locale.get())}
                        </p>
                    }
                }
            >
                <div
                    class="workspace-browser-pick-recent"
                    data-testid="workspace-browser-pick-recent"
                >
                    <For
                        each=move || recent.get()
                        key=|p| p.clone()
                        let:path
                    >
                        {
                            let path_click = path.clone();
                            let path_title = path.clone();
                            let path_label = path.clone();
                            let path_test = path.clone();
                            view! {
                                <button
                                    type="button"
                                    class="btn btn-secondary workspace-project-row"
                                    data-testid="workspace-browser-pick-recent-item"
                                    prop:data-path=path_test
                                    prop:disabled=move || pick.pick_busy_tracked()
                                    prop:title=path_title.clone()
                                    on:click=move |_| {
                                        open_recent_path(open, pick, path_click.clone());
                                    }
                                >
                                    <span class="workspace-project-pool-path">
                                        {workspace_recent_menu_label(&path_label)}
                                    </span>
                                    <span class="workspace-project-open-label">
                                        {move || i18n::ws_project_open(locale.get())}
                                    </span>
                                </button>
                            }
                        }
                    </For>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn WorkspaceBrowserPickPathForm(
    locale: RwSignal<crate::i18n::Locale>,
    path_draft: RwSignal<String>,
    path_input_ref: NodeRef<Input>,
    pick: WorkspaceRootPickHandle,
    open: RwSignal<bool>,
) -> impl IntoView {
    let on_submit = move |_| submit_path(open, pick, path_draft);
    view! {
        <div class="workspace-browser-pick-section">
            <h3 class="workspace-browser-pick-heading">
                {move || i18n::ws_browser_pick_path_heading(locale.get())}
            </h3>
            <label class="settings-field-label" for="workspace-browser-pick-path">
                {move || i18n::ws_path_prompt(locale.get())}
            </label>
            <div class="workspace-browser-pick-path-row">
                <input
                    id="workspace-browser-pick-path"
                    class="input settings-text-input"
                    type="text"
                    autocomplete="off"
                    spellcheck="false"
                    data-testid="workspace-browser-pick-path"
                    node_ref=path_input_ref
                    prop:value=move || path_draft.get()
                    prop:disabled=move || pick.pick_busy_tracked()
                    on:input=move |ev| path_draft.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            submit_path(open, pick, path_draft);
                        }
                    }
                />
                <button
                    type="button"
                    class="btn btn-primary workspace-browser-pick-submit"
                    data-testid="workspace-browser-pick-submit"
                    prop:disabled=move || pick.pick_busy_tracked()
                    on:click=on_submit
                >
                    {move || i18n::ws_browser_pick_submit(locale.get())}
                </button>
            </div>
            <Show when=move || pick.ws.workspace_set_err.get().is_some()>
                <p
                    class="workspace-project-error"
                    role="alert"
                    data-testid="workspace-browser-pick-err"
                >
                    {move || pick.ws.workspace_set_err.get().unwrap_or_default()}
                </p>
            </Show>
        </div>
    }
}

#[component]
fn WorkspaceBrowserPickModalPanel(signals: WorkspaceBrowserPickModalSignals) -> impl IntoView {
    let WorkspaceBrowserPickModalSignals {
        open,
        workspace_pick,
    } = signals;
    let locale = workspace_pick.locale;
    let recent = workspace_pick.ws.recent_workspace_roots;
    let path_draft = RwSignal::new(String::new());
    let dialog_ref = NodeRef::<Div>::new();
    let path_input_ref = NodeRef::<Input>::new();

    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        workspace_pick.ws.workspace_set_err.set(None);
        let current = workspace_pick.ws.workspace_path_draft.get_untracked();
        path_draft.set(current);
        let r = dialog_ref;
        let input = path_input_ref;
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(el) = input.get() {
                let _ = el.focus();
            } else if let Some(el) = r.get() {
                focus_first_in_modal_container(el.as_ref());
            }
        });
    });

    let close = move || open.set(false);

    view! {
        <div
            class="modal"
            node_ref=dialog_ref
            role="dialog"
            aria-modal="true"
            aria-labelledby="workspace-browser-pick-modal-title"
            data-testid="workspace-browser-pick-modal"
            tabindex="-1"
            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Escape" {
                    close();
                } else if ev.key() == "Tab" {
                    if let Some(el) = dialog_ref.get() {
                        trap_tab_in_container(&ev, el.as_ref());
                    }
                }
            }
        >
            <div class="modal-head">
                <h2 class="modal-title" id="workspace-browser-pick-modal-title">
                    {move || i18n::ws_browser_pick_modal_title(locale.get())}
                </h2>
                <span class="modal-head-spacer"></span>
                <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| close()>
                    {move || i18n::settings_close(locale.get())}
                </button>
            </div>
            <div class="modal-body">
                <p class="modal-hint">{move || i18n::ws_browser_pick_modal_hint(locale.get())}</p>
                <WorkspaceBrowserPickRecentList
                    locale=locale
                    recent=recent
                    pick=workspace_pick
                    open=open
                />
                <WorkspaceBrowserPickPathForm
                    locale=locale
                    path_draft=path_draft
                    path_input_ref=path_input_ref
                    pick=workspace_pick
                    open=open
                />
            </div>
        </div>
    }
}

#[component]
fn WorkspaceBrowserPickModalBackdrop(signals: WorkspaceBrowserPickModalSignals) -> impl IntoView {
    let open = signals.open;
    view! {
        <div class="modal-backdrop" on:click=move |_| open.set(false)>
            <WorkspaceBrowserPickModalPanel signals=signals />
        </div>
    }
}

pub fn workspace_browser_pick_modal_view(
    signals: WorkspaceBrowserPickModalSignals,
) -> impl IntoView {
    let open = signals.open;
    view! {
        <Show when=move || open.get()>
            <WorkspaceBrowserPickModalBackdrop signals=signals />
        </Show>
    }
}
