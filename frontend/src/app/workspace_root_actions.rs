//! 工作区根目录选择 / 提交（顶栏只读路径与「文件」菜单共用）。

use leptos::prelude::*;
use leptos::task::spawn_local;

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
    let path = path.clone();
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
            finish_workspace_root_ui(chat, ws, path, loc).await;
        }
        Err(e) => {
            ws.workspace_set_err.set(Some(e));
        }
    }
    ws.workspace_set_busy.set(false);
}

pub(crate) async fn finish_workspace_root_ui(
    chat: ChatSessionSignals,
    ws: WorkspacePanelSignals,
    path: String,
    loc: Locale,
) {
    remember_workspace_root(&path, ws.recent_workspace_roots);
    let aid = chat.active_id.get_untracked();
    patch_active_session_workspace_root(chat.sessions, &aid, path.clone());
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

fn browser_prompt_workspace_path(
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    ws: WorkspacePanelSignals,
    loc: Locale,
) {
    let _ = locale;
    let current = ws.workspace_path_draft.get_untracked();
    let Some(w) = web_sys::window() else {
        return;
    };
    let Ok(Some(raw)) = w.prompt_with_message_and_default(i18n::ws_path_prompt(loc), &current)
    else {
        return;
    };
    let p = raw.trim().to_string();
    if p.is_empty() {
        ws.workspace_set_err
            .set(Some(i18n::ws_path_required(loc).to_string()));
        return;
    }
    ws.workspace_path_draft.set(p.clone());
    ws.workspace_set_busy.set(true);
    spawn_local(async move {
        commit_workspace_root(chat, ws, p, loc).await;
    });
}

impl WorkspaceRootPickHandle {
    /// 桌面壳：系统文件夹对话框并提交；浏览器：提示输入绝对路径后提交。
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
            ws.workspace_pick_busy.set(true);
            let loc = locale.get_untracked();
            spawn_local(async move {
                match crate::api::fetch_workspace_projects(loc).await {
                    Ok(resp) if resp.enabled => {
                        ws.workspace_project_modal_open.set(true);
                    }
                    _ => {
                        browser_prompt_workspace_path(locale, chat, ws, loc);
                    }
                }
                ws.workspace_pick_busy.set(false);
            });
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

/// 顶栏正中：工作区根路径只读标题（切换目录见「文件」菜单）。
#[component]
pub(crate) fn ShellTopbarWorkspaceRoot(pick: WorkspaceRootPickHandle) -> impl IntoView {
    let WorkspaceRootPickHandle { locale, ws, .. } = pick;
    view! {
        <div class="shell-topbar-workspace" data-testid="shell-topbar-workspace">
            <span
                class="shell-topbar-workspace-title"
                data-testid="workspace-root-title"
                prop:aria-label=move || i18n::ws_root_label(locale.get())
                prop:title=move || {
                    let path = ws.workspace_path_draft.get();
                    if path.trim().is_empty() {
                        i18n::ws_path_title_hint(locale.get()).to_string()
                    } else {
                        path
                    }
                }
            >
                {move || {
                    let path = ws.workspace_path_draft.get();
                    if path.trim().is_empty() {
                        i18n::ws_path_empty(locale.get()).to_string()
                    } else {
                        path
                    }
                }}
            </span>
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
