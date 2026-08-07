//! Clone 弹窗表单 / 进度子视图（降低主面板 nloc 与 CCN）。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use crate::api::infer_project_name_from_clone_url;
use crate::app::workspace_root_actions::{WorkspaceRootPickHandle, workspace_inputs_blocked};
use crate::i18n::{self, Locale};

use super::{CloneFormSignals, CloneProgressSignals, CloneUiPhase, start_clone};

#[derive(Clone, Copy)]
pub(super) struct CloneFormViewSignals {
    pub locale: RwSignal<Locale>,
    pub workspace_pick: WorkspaceRootPickHandle,
    pub open: RwSignal<bool>,
    pub url: RwSignal<String>,
    pub name: RwSignal<String>,
    pub shallow: RwSignal<bool>,
    pub branch: RwSignal<String>,
    pub ui_phase: RwSignal<CloneUiPhase>,
    pub status_text: RwSignal<String>,
    pub log_lines: RwSignal<Vec<String>>,
    pub percent: RwSignal<Option<u8>>,
    pub form_err: RwSignal<Option<String>>,
}

#[component]
fn WorkspaceCloneShallowCheck(locale: RwSignal<Locale>, shallow: RwSignal<bool>) -> impl IntoView {
    view! {
        <label class="workspace-clone-field workspace-clone-check">
            <input
                type="checkbox"
                data-testid="workspace-clone-shallow"
                prop:checked=move || shallow.get()
                on:change=move |ev| {
                    if let Some(t) = ev.target() {
                        if let Ok(el) = t.dyn_into::<web_sys::HtmlInputElement>() {
                            shallow.set(el.checked());
                        }
                    }
                }
            />
            <span>{move || i18n::ws_clone_shallow_label(locale.get())}</span>
        </label>
    }
}

#[component]
pub(super) fn WorkspaceCloneFormBody(s: CloneFormViewSignals) -> impl IntoView {
    let CloneFormViewSignals {
        locale,
        workspace_pick,
        open,
        url,
        name,
        shallow,
        branch,
        ui_phase,
        status_text,
        log_lines,
        percent,
        form_err,
    } = s;
    view! {
        <div class="modal-body workspace-clone-form">
            <label class="workspace-clone-field">
                <span>{move || i18n::ws_clone_url_label(locale.get())}</span>
                <input
                    type="text"
                    data-testid="workspace-clone-url"
                    prop:value=move || url.get()
                    prop:placeholder="https://github.com/org/repo.git"
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        url.set(v.clone());
                        if name.get_untracked().trim().is_empty() {
                            name.set(infer_project_name_from_clone_url(&v));
                        }
                    }
                />
            </label>
            <label class="workspace-clone-field">
                <span>{move || i18n::ws_clone_name_label(locale.get())}</span>
                <input
                    type="text"
                    data-testid="workspace-clone-name"
                    prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
            </label>
            <WorkspaceCloneShallowCheck locale=locale shallow=shallow />
            <label class="workspace-clone-field">
                <span>{move || i18n::ws_clone_branch_label(locale.get())}</span>
                <input
                    type="text"
                    data-testid="workspace-clone-branch"
                    prop:value=move || branch.get()
                    on:input=move |ev| branch.set(event_target_value(&ev))
                />
            </label>
            <Show when=move || form_err.get().is_some()>
                <p class="workspace-project-error" role="alert" data-testid="workspace-clone-err">
                    {move || form_err.get().unwrap_or_default()}
                </p>
            </Show>
            <div class="workspace-clone-actions">
                <button
                    type="button"
                    class="btn btn-primary"
                    data-testid="workspace-clone-submit"
                    prop:disabled=move || workspace_inputs_blocked(workspace_pick.ws)
                    on:click=move |_| {
                        start_clone(
                            CloneFormSignals {
                                open,
                                pick: workspace_pick,
                                url,
                                name,
                                shallow,
                                branch,
                            },
                            CloneProgressSignals {
                                ui_phase,
                                status_text,
                                log_lines,
                                percent,
                                form_err,
                            },
                        );
                    }
                >
                    {move || i18n::ws_clone_submit(locale.get())}
                </button>
            </div>
        </div>
    }
}

#[component]
pub(super) fn WorkspaceCloneProgressBody(
    locale: RwSignal<Locale>,
    ui_phase: RwSignal<CloneUiPhase>,
    status_text: RwSignal<String>,
    log_lines: RwSignal<Vec<String>>,
    percent: RwSignal<Option<u8>>,
    form_err: RwSignal<Option<String>>,
    clone_open: RwSignal<bool>,
    settings_page: RwSignal<bool>,
) -> impl IntoView {
    let show_github_cta = move || {
        form_err
            .get()
            .as_deref()
            .is_some_and(|e| e.starts_with("CLONE_AUTH_REQUIRED"))
    };
    view! {
        <div class="modal-body workspace-clone-progress" data-testid="workspace-clone-progress">
            <p class="workspace-clone-status" data-testid="workspace-clone-status">
                {move || status_text.get()}
            </p>
            <div
                class="workspace-clone-bar"
                class:workspace-clone-bar--indeterminate=move || percent.get().is_none()
                role="progressbar"
                prop:aria-valuenow=move || percent.get().map(|p| p.to_string()).unwrap_or_default()
            >
                <div
                    class="workspace-clone-bar-fill"
                    style=move || {
                        percent
                            .get()
                            .map(|p| format!("width:{p}%"))
                            .unwrap_or_else(|| "width:30%".into())
                    }
                ></div>
            </div>
            <pre class="workspace-clone-log" data-testid="workspace-clone-log">
                {move || log_lines.get().join("\n")}
            </pre>
            <Show when=move || form_err.get().is_some()>
                <p class="workspace-project-error" role="alert">
                    {move || form_err.get().unwrap_or_default()}
                </p>
            </Show>
            <Show when=move || ui_phase.get() == CloneUiPhase::Failed>
                <div class="workspace-clone-actions">
                    <button
                        type="button"
                        class="btn btn-secondary"
                        data-testid="workspace-clone-back"
                        on:click=move |_| {
                            ui_phase.set(CloneUiPhase::Form);
                            form_err.set(None);
                        }
                    >
                        {move || i18n::ws_clone_back(locale.get())}
                    </button>
                    <Show when=show_github_cta>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-testid="workspace-clone-connect-github"
                            on:click=move |_| {
                                clone_open.set(false);
                                crate::app::settings_page::navigate_to_settings(
                                    settings_page,
                                    crate::app::settings_page::SettingsSection::Tools,
                                );
                            }
                        >
                            {move || i18n::ws_clone_connect_github(locale.get())}
                        </button>
                    </Show>
                </div>
            </Show>
        </div>
    }
}
