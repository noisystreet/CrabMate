//! 会话列表：首启从 `localStorage` 载入、`GET /web-ui` 一次同步、变更写回。
//!
//! **订阅**：`wire_persist_chat_sessions` 追踪 `sessions` 与 `active_id`——勿在同一 `Effect` 内混入流式高频写入路径以外的无关逻辑。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_web_ui_config;
use crate::i18n::{self, Locale};
use crate::storage::{ChatSession, ensure_at_least_one, load_sessions, save_sessions};

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
        let (list, def_id) = ensure_at_least_one(
            list,
            i18n::default_session_title(locale.get_untracked()).to_string(),
        );
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

/// 会话或活动 id 变化时写回 `localStorage`。
pub fn wire_persist_chat_sessions(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
) {
    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let list = sessions.get();
        let aid = active_id.get();
        if aid.is_empty() {
            return;
        }
        save_sessions(&list, Some(&aid));
    });
}
