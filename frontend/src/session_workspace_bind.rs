//! 会话与 Web 工作区根绑定：`POST /workspace` 成功后写入当前会话；活动会话变化时自动恢复绑定路径。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app::workspace_panel_state::WorkspacePanelSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::ChatSession;

/// 将成功设置的工作区根路径写入当前活动会话（供本地持久化）。
pub fn patch_active_session_workspace_root(
    sessions: RwSignal<Vec<ChatSession>>,
    active_session_id: &str,
    path: String,
) {
    if active_session_id.is_empty() {
        return;
    }
    let p = path.trim().to_string();
    if p.is_empty() {
        return;
    }
    let id = active_session_id.to_string();
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == id) {
            s.workspace_root = Some(p);
        }
    });
}

/// 若 `session_id` 对应会话存有非空 `workspace_root`，则异步 `POST /workspace` 并刷新侧栏目录树。
pub fn spawn_apply_session_bound_workspace(
    sessions: RwSignal<Vec<ChatSession>>,
    session_id: String,
    ws: WorkspacePanelSignals,
    loc: Locale,
) {
    let bound = sessions.with_untracked(|list| {
        list.iter()
            .find(|s| s.id == session_id)
            .and_then(|s| s.workspace_root.as_ref())
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(std::string::ToString::to_string)
    });
    let Some(path) = bound else {
        return;
    };
    spawn_local(async move {
        ws.workspace_set_err.set(None);
        ws.workspace_set_busy.set(true);
        match crate::api::post_workspace_set(Some(path.clone()), loc).await {
            Ok(_) => {
                crate::workspace_shell::reload_workspace_panel(
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

/// 初始化完成且活动会话 id 变化时，应用该会话绑定的工作区（若有）。
pub fn wire_session_bound_workspace_effects(
    initialized: RwSignal<bool>,
    chat: ChatSessionSignals,
    ws: WorkspacePanelSignals,
    locale: RwSignal<Locale>,
) {
    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let id = chat.active_id.get();
        if id.is_empty() {
            return;
        }
        let loc = locale.get_untracked();
        spawn_apply_session_bound_workspace(chat.sessions, id, ws, loc);
    });
}
