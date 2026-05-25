//! 按工作区根路径分桶的会话：服务端 **`workspace_override`** 变化时加载对应 **`/user-data`** 桶。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::api::WorkspaceData;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::{
    ChatSession, clear_stale_assistant_loading_states, ensure_at_least_one,
    normalize_workspace_partition_path,
};
use crate::user_data_bootstrap::load_web_sessions;

fn aid_pick_after_loaded_sessions(
    list2: &mut Vec<ChatSession>,
    aid2: Option<String>,
    active_id: RwSignal<String>,
    sessions: RwSignal<Vec<ChatSession>>,
) -> Option<String> {
    let a = active_id.get_untracked();
    if !a.is_empty() && list2.iter().any(|s| s.id == a) {
        return Some(a);
    }
    if list2.is_empty() && !sessions.get_untracked().is_empty() {
        *list2 = sessions.get_untracked();
        if !a.is_empty() && list2.iter().any(|s| s.id == a) {
            return Some(a);
        }
    }
    aid2
}

fn align_workspace_roots_to_server_path(list2: &mut [ChatSession], server_path: &str) {
    let server_norm = normalize_workspace_partition_path(server_path);
    if server_norm.is_empty() {
        return;
    }
    let canonical = server_path.trim().to_string();
    for s in list2.iter_mut() {
        if let Some(ref wr) = s.workspace_root {
            let wn = normalize_workspace_partition_path(wr);
            if !wn.is_empty() && wn != server_norm {
                s.workspace_root = Some(canonical.clone());
            }
        }
    }
}

/// `wire_workspace_session_storage_partition` 的入参。
#[derive(Clone, Copy)]
pub struct WireWorkspaceSessionPartitionArgs {
    pub initialized: RwSignal<bool>,
    pub workspace_data: RwSignal<Option<WorkspaceData>>,
    pub chat: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub locale: RwSignal<Locale>,
    pub session_workspace_path: RwSignal<String>,
}

/// 在 `workspace_data` 的有效根变化时，从服务端加载另一工作区桶的会话列表。
pub fn wire_workspace_session_storage_partition(args: WireWorkspaceSessionPartitionArgs) {
    let WireWorkspaceSessionPartitionArgs {
        initialized,
        workspace_data,
        chat,
        draft,
        locale,
        session_workspace_path,
    } = args;
    let prev_applied = StoredValue::new(Arc::new(Mutex::new(Option::<String>::None)));

    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let Some(wd) = workspace_data.get() else {
            return;
        };
        if wd.error.is_some() {
            return;
        }
        let norm = normalize_workspace_partition_path(&wd.path);
        let prev_cell = prev_applied.get_value();
        let mut prev_slot = prev_cell.lock().expect("partition prev workspace");
        if prev_slot.as_deref() == Some(norm.as_str()) {
            return;
        }
        *prev_slot = Some(norm.clone());
        drop(prev_slot);

        let sessions = chat.sessions;
        let active_id = chat.active_id;
        let overlay = chat.stream_text_overlay;
        let loc = locale.get_untracked();

        leptos::task::spawn_local(async move {
            let (mut list2, aid2) = load_web_sessions(loc).await;
            let aid_for_pick =
                aid_pick_after_loaded_sessions(&mut list2, aid2, active_id, sessions);

            align_workspace_roots_to_server_path(&mut list2, wd.path.as_str());

            for s in list2.iter_mut() {
                clear_stale_assistant_loading_states(&mut s.messages);
            }
            let (list2, def_id) =
                ensure_at_least_one(list2, crate::i18n::default_session_title(loc).to_string());
            let pick = aid_for_pick
                .filter(|id| list2.iter().any(|s| s.id == *id))
                .unwrap_or(def_id);
            let d = list2
                .iter()
                .find(|s| s.id == pick)
                .map(|s| s.draft.clone())
                .unwrap_or_default();

            chat.clear_stream_resume_handles();
            overlay.set(None);
            chat.session_sync
                .set(crate::session_sync::SessionSyncState::local_only());
            chat.reasoning_preserved.set(HashMap::new());
            chat.session_hydrate_nonce
                .update(|n| *n = n.wrapping_add(1));

            sessions.set(list2);
            active_id.set(pick);
            draft.set(d);
            session_workspace_path.set(norm);
        });
    });
}
