//! 工作区根目录选择 / 提交（顶栏路径框与「文件」菜单共用）。

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::api::post_workspace_set;
use crate::api::user_data::put_current_web_sessions;
use crate::app_prefs::SidePanelView;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_export::tauri_pick_workspace_folder;
use crate::session_workspace_bind::patch_active_session_workspace_root;
use crate::stream_text_overlay::sessions_snapshot_with_stream_overlay_merged;
use crate::tauri_shell::tauri_shell_available;
use crate::user_data_bootstrap::remember_workspace_root;
use crate::workspace_shell::reload_workspace_panel;

use super::workspace_panel_state::WorkspacePanelSignals;

/// 顶栏 / 菜单共用的工作区根选择句柄。
#[derive(Clone, Copy)]
pub struct WorkspaceRootPickHandle {
    pub locale: RwSignal<Locale>,
    pub chat: ChatSessionSignals,
    pub ws: WorkspacePanelSignals,
    pub side_panel_view: RwSignal<SidePanelView>,
}

pub(crate) fn workspace_inputs_blocked(ws: WorkspacePanelSignals) -> bool {
    ws.workspace_set_busy.get() || ws.workspace_pick_busy.get() || ws.workspace_loading.get()
}

pub(crate) async fn commit_workspace_root(
    chat: ChatSessionSignals,
    ws: WorkspacePanelSignals,
    path: String,
    loc: Locale,
) {
    let path_for_bind = path.clone();
    let aid = chat.active_id.get_untracked();
    if !aid.is_empty() {
        let list = chat.sessions.get_untracked();
        let merged = sessions_snapshot_with_stream_overlay_merged(
            list.as_slice(),
            chat.stream_text_overlay.get_untracked().as_ref(),
        );
        let _ = put_current_web_sessions(&merged, Some(aid.as_str()), loc).await;
    }
    match post_workspace_set(Some(path.clone()), loc).await {
        Ok(_) => {
            remember_workspace_root(&path, ws.recent_workspace_roots);
            let aid = chat.active_id.get_untracked();
            patch_active_session_workspace_root(chat.sessions, &aid, path_for_bind);
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
}

fn focus_workspace_root_input() {
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Ok(Some(el)) = doc.query_selector("[data-testid=\"workspace-root-input\"]") else {
            return;
        };
        if let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = html.focus();
        }
    });
}

impl WorkspaceRootPickHandle {
    /// 桌面壳：系统文件夹对话框并提交；浏览器：聚焦顶栏路径框。
    pub fn spawn_pick_or_reveal(self) {
        let Self {
            locale,
            chat,
            ws,
            side_panel_view,
        } = self;
        ws.workspace_set_err.set(None);
        if workspace_inputs_blocked(ws) {
            return;
        }
        if !tauri_shell_available() {
            side_panel_view.set(SidePanelView::Workspace);
            focus_workspace_root_input();
            return;
        }
        ws.workspace_pick_busy.set(true);
        let loc = locale.get_untracked();
        spawn_local(async move {
            match tauri_pick_workspace_folder().await {
                Ok(None) => {}
                Ok(Some(raw)) => {
                    let p = raw.trim().to_string();
                    if !p.is_empty() {
                        ws.workspace_path_draft.set(p.clone());
                        ws.workspace_set_busy.set(true);
                        commit_workspace_root(chat, ws, p, loc).await;
                    }
                }
                Err(e) => {
                    ws.workspace_set_err.set(Some(e));
                }
            }
            ws.workspace_pick_busy.set(false);
        });
    }

    #[must_use]
    pub fn pick_busy_tracked(&self) -> bool {
        workspace_inputs_blocked(self.ws)
    }

    #[must_use]
    pub fn menu_label(&self) -> &'static str {
        let loc = self.locale.get();
        if self.ws.workspace_pick_busy.get() {
            i18n::ws_browse_busy_title(loc)
        } else {
            i18n::ide_menu_open_workspace(loc)
        }
    }

    /// 从最近列表打开已记录路径（不打开系统对话框）。
    pub fn spawn_open_recent(self, path: String) {
        let Self {
            locale, chat, ws, ..
        } = self;
        ws.workspace_set_err.set(None);
        let p = path.trim().to_string();
        if p.is_empty() || workspace_inputs_blocked(ws) {
            return;
        }
        ws.workspace_path_draft.set(p.clone());
        ws.workspace_set_busy.set(true);
        let loc = locale.get_untracked();
        spawn_local(async move {
            commit_workspace_root(chat, ws, p, loc).await;
        });
    }
}

/// 顶栏正中：工作区根路径（手输 Enter 提交；选目录见「文件」菜单）。
#[component]
pub(crate) fn ShellTopbarWorkspaceRoot(pick: WorkspaceRootPickHandle) -> impl IntoView {
    let WorkspaceRootPickHandle {
        locale, chat, ws, ..
    } = pick;
    view! {
        <div class="shell-topbar-workspace" data-testid="shell-topbar-workspace">
            <input
                type="text"
                class="shell-topbar-workspace-input"
                data-testid="workspace-root-input"
                prop:placeholder=move || i18n::ws_input_ph(locale.get())
                prop:title=move || i18n::ws_input_title(locale.get())
                prop:aria-label=move || i18n::ws_root_label(locale.get())
                prop:value=move || ws.workspace_path_draft.get()
                prop:disabled=move || workspace_inputs_blocked(ws)
                on:input=move |ev| {
                    ws.workspace_path_draft.set(event_target_value(&ev));
                }
                on:keydown=move |ev: KeyboardEvent| {
                    if ev.key() != "Enter" {
                        return;
                    }
                    ev.prevent_default();
                    ws.workspace_set_err.set(None);
                    let p = ws.workspace_path_draft.get().trim().to_string();
                    if p.is_empty() {
                        ws.workspace_set_err.set(Some(
                            i18n::ws_path_required(locale.get()).to_string(),
                        ));
                        return;
                    }
                    if workspace_inputs_blocked(ws) {
                        return;
                    }
                    ws.workspace_set_busy.set(true);
                    let loc = locale.get_untracked();
                    spawn_local(async move {
                        commit_workspace_root(chat, ws, p, loc).await;
                    });
                }
            />
            <Show when=move || ws.workspace_set_err.get().is_some()>
                <span class="shell-topbar-workspace-error" role="alert" prop:title=move || {
                    ws.workspace_set_err.get().unwrap_or_default()
                }>
                    {move || ws.workspace_set_err.get().unwrap_or_default()}
                </span>
            </Show>
        </div>
    }
}
