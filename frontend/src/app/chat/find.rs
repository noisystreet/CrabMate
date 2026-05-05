//! 主区会话内查找：匹配 id 列表、光标与首条滚入视图。

use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::i18n::Locale;
use crate::message_format::message_text_for_display_ex;
use crate::session_search::{normalize_search_query, scroll_message_into_view};
use crate::storage::ChatSession;

#[allow(clippy::too_many_arguments)]
pub(crate) fn wire_chat_find_matches(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    auto_scroll_chat: RwSignal<bool>,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
) {
    // 上一次用于生成匹配列表的 `(规范化查询, 活动会话 id)`；非响应式，仅避免同键重复重置光标。
    let chat_find_prev_key = StoredValue::new((String::new(), String::new()));
    Effect::new({
        let sessions = sessions;
        let active_id = active_id;
        let chat_find_query = chat_find_query;
        let chat_find_match_ids = chat_find_match_ids;
        let chat_find_cursor = chat_find_cursor;
        let auto_scroll_chat = auto_scroll_chat;
        let locale = locale;
        let apply_assistant_display_filters = apply_assistant_display_filters;
        move |_| {
            let aid = active_id.get();
            let loc = locale.get();
            let apply = apply_assistant_display_filters.get();
            let q = normalize_search_query(&chat_find_query.get());
            let ids = if q.is_empty() {
                Vec::new()
            } else {
                sessions.with(|list| {
                    list.iter()
                        .find(|s| s.id == aid)
                        .map(|s| {
                            s.messages
                                .iter()
                                .filter(|m| {
                                    message_text_for_display_ex(m, loc, apply)
                                        .to_lowercase()
                                        .contains(&q)
                                })
                                .map(|m| m.id.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
            };
            chat_find_match_ids.set(ids.clone());
            let key_changed = chat_find_prev_key
                .try_update_value(|prev| {
                    let key_changed = prev.0 != q || prev.1 != aid;
                    if key_changed {
                        prev.0 = q.clone();
                        prev.1 = aid.clone();
                    }
                    key_changed
                })
                .unwrap_or(true);
            if key_changed {
                chat_find_cursor.set(0);
                if !q.is_empty() && !ids.is_empty() {
                    auto_scroll_chat.set(false);
                    let first = ids[0].clone();
                    spawn_local(async move {
                        TimeoutFuture::new(32).await;
                        scroll_message_into_view(&first);
                    });
                }
            } else {
                chat_find_cursor.update(|c| {
                    if ids.is_empty() {
                        *c = 0;
                    } else if *c >= ids.len() {
                        *c = ids.len() - 1;
                    }
                });
            }
        }
    });
}
