//! 项目池：Clone 远程仓库弹窗（表单 → SSE 进度 → 切换工作区）。

mod parts;

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::api::{
    WorkspaceCloneRequest, WorkspaceCloneSseEvent, fetch_workspace_projects,
    post_workspace_clone_stream,
};
use crate::app::workspace_root_actions::{
    WorkspaceRootPickHandle, finish_workspace_root_ui, workspace_inputs_blocked,
};
use crate::i18n::{self, Locale};

use parts::{CloneFormViewSignals, WorkspaceCloneFormBody, WorkspaceCloneProgressBody};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CloneUiPhase {
    Form,
    Running,
    Failed,
    Succeeded,
}

#[derive(Clone, Copy)]
pub struct WorkspaceCloneModalSignals {
    pub open: RwSignal<bool>,
    pub workspace_pick: WorkspaceRootPickHandle,
    /// 跳转「设置 → 工具」（GitHub 连接）用。
    pub settings_page: RwSignal<bool>,
}

fn phase_label(locale: Locale, phase: &str) -> &'static str {
    match phase {
        "validate" => i18n::ws_clone_phase_validate(locale),
        "activate" => i18n::ws_clone_phase_activate(locale),
        _ => i18n::ws_clone_phase_clone(locale),
    }
}

#[derive(Clone, Copy)]
pub(super) struct CloneFormSignals {
    pub open: RwSignal<bool>,
    pub pick: WorkspaceRootPickHandle,
    pub url: RwSignal<String>,
    pub name: RwSignal<String>,
    pub shallow: RwSignal<bool>,
    pub branch: RwSignal<String>,
}

#[derive(Clone, Copy)]
pub(super) struct CloneProgressSignals {
    pub ui_phase: RwSignal<CloneUiPhase>,
    pub status_text: RwSignal<String>,
    pub log_lines: RwSignal<Vec<String>>,
    pub percent: RwSignal<Option<u8>>,
    pub form_err: RwSignal<Option<String>>,
}

pub(super) fn start_clone(form: CloneFormSignals, progress: CloneProgressSignals) {
    let CloneFormSignals {
        open,
        pick,
        url,
        name,
        shallow,
        branch,
    } = form;
    let CloneProgressSignals {
        ui_phase,
        status_text,
        log_lines,
        percent,
        form_err,
    } = progress;
    if workspace_inputs_blocked(pick.ws) || ui_phase.get_untracked() == CloneUiPhase::Running {
        return;
    }
    let loc = pick.locale.get_untracked();
    let u = url.get_untracked().trim().to_string();
    let n = name.get_untracked().trim().to_string();
    if u.is_empty() || n.is_empty() {
        form_err.set(Some(i18n::ws_clone_need_fields(loc).to_string()));
        return;
    }
    form_err.set(None);
    ui_phase.set(CloneUiPhase::Running);
    status_text.set(i18n::ws_clone_phase_validate(loc).to_string());
    log_lines.set(Vec::new());
    percent.set(None);
    pick.ws.workspace_set_busy.set(true);

    let depth = if shallow.get_untracked() {
        Some(1u32)
    } else {
        None
    };
    let br = {
        let b = branch.get_untracked().trim().to_string();
        if b.is_empty() { None } else { Some(b) }
    };
    let req = WorkspaceCloneRequest {
        url: u,
        name: n,
        depth,
        branch: br,
    };

    spawn_local(async move {
        let result = post_workspace_clone_stream(req, loc, |ev| match ev {
            WorkspaceCloneSseEvent::Phase(p) => {
                status_text.set(phase_label(loc, &p).to_string());
            }
            WorkspaceCloneSseEvent::Log(line) => {
                log_lines.update(|v| {
                    v.push(line);
                    if v.len() > 200 {
                        let drain = v.len() - 200;
                        v.drain(..drain);
                    }
                });
            }
            WorkspaceCloneSseEvent::Progress { percent: p, .. } => {
                percent.set(Some(p));
            }
            WorkspaceCloneSseEvent::Done { .. } | WorkspaceCloneSseEvent::Error { .. } => {}
        })
        .await;

        match result {
            Ok((_name, path)) => {
                status_text.set(i18n::ws_clone_done(loc).to_string());
                percent.set(Some(100));
                finish_workspace_root_ui(pick.chat, pick.ws, path, loc).await;
                pick.ws.workspace_set_busy.set(false);
                ui_phase.set(CloneUiPhase::Succeeded);
                TimeoutFuture::new(400).await;
                open.set(false);
                ui_phase.set(CloneUiPhase::Form);
            }
            Err(e) => {
                pick.ws.workspace_set_busy.set(false);
                form_err.set(Some(e));
                ui_phase.set(CloneUiPhase::Failed);
            }
        }
    });
}

fn reset_clone_form_state(form: CloneFormSignals, progress: CloneProgressSignals) {
    form.url.set(String::new());
    form.name.set(String::new());
    form.shallow.set(true);
    form.branch.set(String::new());
    progress.ui_phase.set(CloneUiPhase::Form);
    progress.form_err.set(None);
    progress.log_lines.set(Vec::new());
    progress.percent.set(None);
}

#[component]
fn WorkspaceCloneModalPanel(signals: WorkspaceCloneModalSignals) -> impl IntoView {
    let WorkspaceCloneModalSignals {
        open,
        workspace_pick,
        settings_page,
    } = signals;
    let locale = workspace_pick.locale;
    let url = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let shallow = RwSignal::new(true);
    let branch = RwSignal::new(String::new());
    let ui_phase = RwSignal::new(CloneUiPhase::Form);
    let status_text = RwSignal::new(String::new());
    let log_lines = RwSignal::new(Vec::<String>::new());
    let percent = RwSignal::new(None::<u8>);
    let form_err = RwSignal::new(None::<String>);
    let dialog_ref = NodeRef::<Div>::new();

    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        reset_clone_form_state(
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
        let r = dialog_ref;
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(el) = r.get() {
                focus_first_in_modal_container(el.as_ref());
            }
        });
    });

    let try_close = move || {
        if ui_phase.get_untracked() == CloneUiPhase::Running {
            return;
        }
        open.set(false);
    };

    let form_signals = CloneFormViewSignals {
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
    };

    view! {
        <div
            class="modal workspace-clone-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="workspace-clone-modal-title"
            data-testid="workspace-clone-modal"
            tabindex="-1"
            node_ref=dialog_ref
            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Escape" {
                    try_close();
                } else if ev.key() == "Tab" {
                    if let Some(el) = dialog_ref.get() {
                        trap_tab_in_container(&ev, el.as_ref());
                    }
                }
            }
        >
            <div class="modal-head">
                <h2 class="modal-title" id="workspace-clone-modal-title">
                    {move || i18n::ws_clone_modal_title(locale.get())}
                </h2>
                <span class="modal-head-spacer"></span>
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    prop:disabled=move || ui_phase.get() == CloneUiPhase::Running
                    on:click=move |_| try_close()
                >
                    {move || i18n::settings_close(locale.get())}
                </button>
            </div>
            <Show when=move || matches!(ui_phase.get(), CloneUiPhase::Form)>
                <WorkspaceCloneFormBody s=form_signals />
            </Show>
            <Show when=move || !matches!(ui_phase.get(), CloneUiPhase::Form)>
                <WorkspaceCloneProgressBody
                    locale=locale
                    ui_phase=ui_phase
                    status_text=status_text
                    log_lines=log_lines
                    percent=percent
                    form_err=form_err
                    clone_open=open
                    settings_page=settings_page
                />
            </Show>
        </div>
    }
}

#[component]
fn WorkspaceCloneModalBackdrop(signals: WorkspaceCloneModalSignals) -> impl IntoView {
    let open = signals.open;
    view! {
        <div
            class="modal-backdrop"
            data-testid="workspace-clone-backdrop"
            on:click=move |_| {
                if signals.workspace_pick.ws.workspace_set_busy.get_untracked() {
                    return;
                }
                open.set(false);
            }
        >
            <WorkspaceCloneModalPanel signals=signals />
        </div>
    }
}

pub fn workspace_clone_modal_view(signals: WorkspaceCloneModalSignals) -> impl IntoView {
    let open = signals.open;
    view! {
        <Show when=move || open.get()>
            <WorkspaceCloneModalBackdrop signals=signals />
        </Show>
    }
}

/// 探测项目池是否启用（菜单显示 Clone 项）。
pub fn spawn_refresh_workspace_pool_enabled(
    pool_enabled: RwSignal<bool>,
    locale: RwSignal<Locale>,
) {
    spawn_local(async move {
        let loc = locale.get_untracked();
        match fetch_workspace_projects(loc).await {
            Ok(r) => pool_enabled.set(r.enabled),
            Err(_) => pool_enabled.set(false),
        }
    });
}
