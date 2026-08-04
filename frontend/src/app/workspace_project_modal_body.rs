//! 项目池弹窗：列表区与加载状态（降低主面板圈复杂度）。

use std::sync::Arc;

use leptos::prelude::*;

use crate::app::workspace_panel_state::WorkspacePanelSignals;
use crate::i18n::{self, Locale};

use super::workspace_project_modal_parts::WorkspaceProjectListRows;

#[component]
pub(crate) fn WorkspaceProjectModalBody(
    locale: RwSignal<Locale>,
    ws: WorkspacePanelSignals,
    projects: RwSignal<Vec<String>>,
    pool_path: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
    load_err: RwSignal<Option<String>>,
    on_open: Arc<dyn Fn(String) + Send + Sync>,
) -> impl IntoView {
    view! {
        <p class="modal-hint">{move || i18n::ws_project_modal_hint(locale.get())}</p>
        <Show when=move || pool_path.get().is_some()>
            <p class="modal-hint workspace-project-pool-path">
                {move || pool_path.get().unwrap_or_default()}
            </p>
        </Show>
        <Show when=move || load_err.get().is_some()>
            <p class="shell-topbar-workspace-error" role="alert">
                {move || load_err.get().unwrap_or_default()}
            </p>
        </Show>
        <Show when=move || loading.get()>
            <p class="modal-hint">{move || i18n::ws_project_loading(locale.get())}</p>
        </Show>
        <Show when=move || !loading.get() && load_err.get().is_none()>
            <WorkspaceProjectListRows
                locale=locale
                ws=ws
                projects=projects
                on_open=Arc::clone(&on_open)
            />
        </Show>
    }
}

pub(crate) fn spawn_reload_workspace_projects(
    locale: RwSignal<Locale>,
    projects: RwSignal<Vec<String>>,
    pool_path: RwSignal<Option<String>>,
    loading: RwSignal<bool>,
    load_err: RwSignal<Option<String>>,
) {
    loading.set(true);
    load_err.set(None);
    let loc = locale.get_untracked();
    leptos::task::spawn_local(async move {
        match crate::api::fetch_workspace_projects(loc).await {
            Ok(resp) => {
                pool_path.set(resp.pool_path);
                projects.set(resp.projects);
                loading.set(false);
            }
            Err(e) => {
                load_err.set(Some(e));
                loading.set(false);
            }
        }
    });
}
