//! 会话列表：首启从 `localStorage` 载入、`GET /web-ui` 一次同步、变更写回。
//!
//! **订阅**：`wire_persist_chat_sessions` 追踪 `sessions` 与 `active_id`——勿在同一 `Effect` 内混入流式高频写入路径以外的无关逻辑。
//! 写盘经 **防抖**（[`PERSIST_SESSIONS_DEBOUNCE_MS`]）：流式正文高频更新时合并为单次 `save_sessions`，减轻主线程与 `localStorage` 压力。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_web_ui_config;
use crate::i18n::{self, Locale};
use crate::storage::{
    ChatSession, clear_stale_assistant_loading_states, ensure_at_least_one, load_sessions,
    save_sessions,
};

/// 首次渲染时从 `localStorage` 加载会话列表并设活动会话与草稿。
pub fn wire_initial_sessions_from_storage(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    locale: RwSignal<Locale>,
) {
    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let (list, aid) = load_sessions();
        let (mut list, def_id) = ensure_at_least_one(
            list,
            i18n::default_session_title(locale.get_untracked()).to_string(),
        );
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

/// 会话列表写回 `localStorage` 的防抖间隔（毫秒）。
const PERSIST_SESSIONS_DEBOUNCE_MS: u32 = 400;

/// 会话或活动 id 变化时写回 `localStorage`（防抖：安静窗口后落盘最新快照）。
pub fn wire_persist_chat_sessions(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
) {
    let debounce_tick = StoredValue::new(Arc::new(AtomicU64::new(0)));
    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let _ = sessions.get();
        let _ = active_id.get();
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
            save_sessions(&list, Some(&aid));
        });
    });
}
