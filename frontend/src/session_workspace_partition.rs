//! 按工作区根路径分桶的本地会话存储：`GET /workspace` 返回的根路径变化时，保存当前桶并加载另一桶的会话列表。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::api::WorkspaceData;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::storage::{
    ChatSession, SESSIONS_KEY_LEGACY, clear_stale_assistant_loading_states, ensure_at_least_one,
    load_sessions_at_storage_key, normalize_workspace_partition_path, save_sessions_at_storage_key,
    sessions_json_storage_key,
};
use crate::stream_text_overlay::{StreamTextOverlay, sessions_snapshot_with_stream_overlay_merged};

/// 与上次已应用桶相同、或完成 legacy 首次绑定时返回 `None`（Effect 直接 return）。
fn partition_prev_key_for_save_or_skip(
    prev_cell: &Arc<Mutex<Option<String>>>,
    new_key: &str,
    session_sessions_storage_key: RwSignal<String>,
) -> Option<String> {
    let mut prev_slot = prev_cell.lock().expect("partition prev key mutex");
    if prev_slot.as_deref() == Some(new_key) {
        return None;
    }
    if prev_slot.is_none() && new_key == SESSIONS_KEY_LEGACY {
        session_sessions_storage_key.set(SESSIONS_KEY_LEGACY.to_string());
        *prev_slot = Some(SESSIONS_KEY_LEGACY.to_string());
        return None;
    }
    Some(
        prev_slot
            .clone()
            .unwrap_or_else(|| SESSIONS_KEY_LEGACY.to_string()),
    )
}

fn persist_sessions_before_partition_switch(
    prev_key_for_save: &str,
    active_id: &str,
    sessions: RwSignal<Vec<ChatSession>>,
    overlay: RwSignal<Option<StreamTextOverlay>>,
) {
    if active_id.is_empty() {
        return;
    }
    let list = sessions.get_untracked();
    let merged = sessions_snapshot_with_stream_overlay_merged(
        list.as_slice(),
        overlay.get_untracked().as_ref(),
    );
    save_sessions_at_storage_key(prev_key_for_save, &merged, Some(active_id));
}

fn aid_pick_after_loaded_sessions(
    seed_from_ram: bool,
    list2: &mut Vec<ChatSession>,
    aid2: Option<String>,
    active_id: RwSignal<String>,
    sessions: RwSignal<Vec<ChatSession>>,
) -> Option<String> {
    if !seed_from_ram {
        return aid2;
    }
    let a = active_id.get_untracked();
    *list2 = sessions.get_untracked();
    if a.is_empty() || !list2.iter().any(|s| s.id == a) {
        None
    } else {
        Some(a)
    }
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

/// `wire_workspace_session_storage_partition` 的入参（避免形参棘轮）。
#[derive(Clone, Copy)]
pub struct WireWorkspaceSessionPartitionArgs {
    pub initialized: RwSignal<bool>,
    pub workspace_data: RwSignal<Option<WorkspaceData>>,
    pub chat: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub locale: RwSignal<Locale>,
    pub session_sessions_storage_key: RwSignal<String>,
}

/// 在 `workspace_data` 所代表的**有效**工作区根变化时，切换 `localStorage` 会话桶。
pub fn wire_workspace_session_storage_partition(args: WireWorkspaceSessionPartitionArgs) {
    let WireWorkspaceSessionPartitionArgs {
        initialized,
        workspace_data,
        chat,
        draft,
        locale,
        session_sessions_storage_key,
    } = args;
    let prev_applied_key = StoredValue::new(Arc::new(Mutex::new(Option::<String>::None)));

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
        let new_key = sessions_json_storage_key(wd.path.as_str());
        let prev_cell = prev_applied_key.get_value();
        let Some(prev_key_for_save) = partition_prev_key_for_save_or_skip(
            &prev_cell,
            new_key.as_str(),
            session_sessions_storage_key,
        ) else {
            return;
        };

        let sessions = chat.sessions;
        let active_id = chat.active_id;
        let overlay = chat.stream_text_overlay;

        persist_sessions_before_partition_switch(
            prev_key_for_save.as_str(),
            active_id.get_untracked().as_str(),
            sessions,
            overlay,
        );

        let (mut list2, aid2) = load_sessions_at_storage_key(&new_key);
        let seed_from_ram = prev_key_for_save == SESSIONS_KEY_LEGACY
            && new_key != SESSIONS_KEY_LEGACY
            && list2.is_empty();
        let aid_for_pick =
            aid_pick_after_loaded_sessions(seed_from_ram, &mut list2, aid2, active_id, sessions);

        // 本会话桶由 `wd.path` 派生：若某条会话仍带着「另一套工作区根」的残留绑定，会在
        // `wire_session_bound_workspace_effects` 里反复 `POST /workspace` ↔ 本分桶 Effect 之间振荡。
        align_workspace_roots_to_server_path(&mut list2, wd.path.as_str());

        for s in list2.iter_mut() {
            clear_stale_assistant_loading_states(&mut s.messages);
        }
        let loc = locale.get_untracked();
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
        session_sessions_storage_key.set(new_key.clone());
        *prev_cell.lock().expect("partition prev key mutex") = Some(new_key);
    });
}
