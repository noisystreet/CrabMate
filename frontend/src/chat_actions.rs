//! 聊天侧「服务端分支」与会话 revision 对齐（无 UI、无 `view!`）。
//!
//! 与 `crate::api::http::post_chat_branch` 配合：HTTP 在 `api`，成功后对 `SessionSyncState` / `ChatSession` 的写入集中在此模块。

use leptos::prelude::*;

use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

/// `POST /chat/branch` 成功后，更新 `session_sync` 与活动会话的 `server_revision`。
pub fn apply_branch_success_revision(
    session_sync: RwSignal<SessionSyncState>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_session_id: &str,
    new_revision: u64,
) {
    session_sync.update(|s| s.set_revision_after_branch(new_revision));
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|x| x.id == active_session_id) {
            s.server_revision = Some(new_revision);
        }
    });
}
