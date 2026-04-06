//! 当前会话内查找工具条。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::session_search::scroll_message_into_view;

#[component]
pub fn ChatFindBar(
    chat_find_panel_open: RwSignal<bool>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    auto_scroll_chat: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="chat-find-wrap">
            <div class="chat-find-bar" role="search" aria-label="在当前会话中查找">
                <label class="chat-find-label" for="chat-find-input">"查找"</label>
                <input
                    id="chat-find-input"
                    type="search"
                    class="chat-find-input"
                    placeholder="当前会话消息…"
                    prop:value=move || chat_find_query.get()
                    on:input=move |ev| {
                        chat_find_query.set(event_target_value(&ev));
                    }
                />
                <span class="chat-find-meta" aria-live="polite">
                    {move || {
                        let q = chat_find_query.get();
                        if q.trim().is_empty() {
                            return String::new();
                        }
                        let n = chat_find_match_ids.with(|v| v.len());
                        let c = chat_find_cursor.get();
                        if n == 0 {
                            "无匹配".to_string()
                        } else {
                            format!("{} / {}", c + 1, n)
                        }
                    }}
                </span>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-nav"
                    title="上一条匹配"
                    prop:disabled=move || {
                        chat_find_query.get().trim().is_empty()
                            || chat_find_match_ids.with(|v| v.is_empty())
                    }
                    on:click=move |_| {
                        let ids = chat_find_match_ids.get();
                        if ids.is_empty() {
                            return;
                        }
                        auto_scroll_chat.set(false);
                        chat_find_cursor.update(|i| {
                            if *i == 0 {
                                *i = ids.len() - 1;
                            } else {
                                *i -= 1;
                            }
                        });
                        let idx = chat_find_cursor.get();
                        scroll_message_into_view(&ids[idx]);
                    }
                >
                    "↑"
                </button>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-nav"
                    title="下一条匹配"
                    prop:disabled=move || {
                        chat_find_query.get().trim().is_empty()
                            || chat_find_match_ids.with(|v| v.is_empty())
                    }
                    on:click=move |_| {
                        let ids = chat_find_match_ids.get();
                        if ids.is_empty() {
                            return;
                        }
                        auto_scroll_chat.set(false);
                        chat_find_cursor.update(|i| {
                            *i = (*i + 1) % ids.len();
                        });
                        let idx = chat_find_cursor.get();
                        scroll_message_into_view(&ids[idx]);
                    }
                >
                    "↓"
                </button>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-close"
                    title="收起查找栏"
                    aria-label="收起查找栏"
                    on:click=move |_| chat_find_panel_open.set(false)
                >
                    "×"
                </button>
            </div>
        </div>
    }
}
