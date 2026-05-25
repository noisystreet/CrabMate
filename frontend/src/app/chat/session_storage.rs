//! 会话列表：首启从 **`/user-data`** 载入、`GET /web-ui` 一次同步、变更写回服务端。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_web_ui_config;
use crate::api::user_data::put_current_web_sessions;
use crate::chat_session_state::ChatSessionSignals;

use super::session_hydrate::bump_session_hydrate_nonce;
use crate::api::client_llm_storage::hydrate_client_llm_from_server;
use crate::i18n::{self, Locale};
use crate::storage::{clear_stale_assistant_loading_states, ensure_at_least_one};
use crate::stream_text_overlay::sessions_snapshot_with_stream_overlay_merged;
use crate::user_data_bootstrap::load_web_sessions;
use crate::user_prefs_sync::wire_load_user_prefs_from_server;

const PERSIST_SESSIONS_DEBOUNCE_MS: u32 = 400;

/// 首次渲染时从 `/user-data` 加载会话列表并设活动会话与草稿。
pub fn wire_initial_sessions_from_storage(app: crate::app::app_signals::AppSignals) {
    let initialized = app.initialized;
    let sessions = app.chat.sessions;
    let active_id = app.chat.active_id;
    let draft = app.chat_composer.draft;
    let locale = app.shell_ui.locale;
    let chat = app.chat;

    wire_load_user_prefs_from_server(app.clone());
    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let loc = locale.get_untracked();
        spawn_local(async move {
            hydrate_client_llm_from_server(loc).await;
            let (list, aid) = load_web_sessions(loc).await;
            let (mut list, def_id) =
                ensure_at_least_one(list, i18n::default_session_title(loc).to_string());
            for s in &mut list {
                clear_stale_assistant_loading_states(&mut s.messages);
            }
            let pick = aid
                .filter(|id| list.iter().any(|s| s.id == *id))
                .unwrap_or(def_id);
            let d = list
                .iter()
                .find(|s| s.id == pick)
                .map(|s| s.draft.clone())
                .unwrap_or_default();
            sessions.set(list);
            active_id.set(pick);
            draft.set(d);
            initialized.set(true);
            bump_session_hydrate_nonce(chat);
        });
    });
}

/// 初始化完成后拉取一次 **`GET /web-ui`**，同步 Markdown / 助手过滤开关。
pub fn wire_web_ui_config_once_after_init(
    initialized: RwSignal<bool>,
    web_ui_config_loaded: RwSignal<bool>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
    locale: RwSignal<Locale>,
) {
    Effect::new({
        move |_| {
            if !initialized.get() || web_ui_config_loaded.get() {
                return;
            }
            web_ui_config_loaded.set(true);
            let locale_val = locale.get_untracked();
            spawn_local(async move {
                if let Ok(c) = fetch_web_ui_config(locale_val).await {
                    markdown_render.set(c.markdown_render);
                    apply_assistant_display_filters.set(c.apply_assistant_display_filters);
                }
            });
        }
    });
}

/// 会话或活动 id 变化时写回 **`PUT /user-data/workspaces/current/sessions`**（防抖）。
pub fn wire_persist_chat_sessions(
    initialized: RwSignal<bool>,
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
) {
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let stream_text_overlay = chat.stream_text_overlay;
    let debounce_tick = StoredValue::new(Arc::new(AtomicU64::new(0)));
    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let _ = sessions.get();
        let _ = active_id.get();
        let _ = stream_text_overlay.get();
        let ctr = debounce_tick.get_value();
        let prev = ctr.fetch_add(1, Ordering::Relaxed);
        let tick = prev.wrapping_add(1);
        let ctr2 = Arc::clone(&ctr);
        spawn_local(async move {
            TimeoutFuture::new(PERSIST_SESSIONS_DEBOUNCE_MS).await;
            if ctr2.load(Ordering::Relaxed) != tick {
                return;
            }
            if !initialized.get_untracked() {
                return;
            }
            let list = sessions.get_untracked();
            let aid = active_id.get_untracked();
            if aid.is_empty() {
                return;
            }
            let merged = sessions_snapshot_with_stream_overlay_merged(
                list.as_slice(),
                stream_text_overlay.get_untracked().as_ref(),
            );
            let loc = locale.get_untracked();
            let _ = put_current_web_sessions(&merged, Some(aid.as_str()), loc).await;
        });
    });
}
