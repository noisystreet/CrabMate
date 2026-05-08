//! 当前会话内查找工具条。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::i18n::{self, Locale};
use crate::session_search::scroll_message_into_view;

/// 查找条所需 `RwSignal` 聚合（阶段 B：压缩 `App` 的 `view!` 实参）。
#[derive(Clone, Copy)]
pub struct ChatFindBarSignals {
    pub chat_find_panel_open: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub auto_scroll_chat: RwSignal<bool>,
}

enum ChatFindNavDir {
    Prev,
    Next,
}

fn chat_find_meta_line(locale: Locale, query: &str, match_ids: &[String], cursor: usize) -> String {
    if query.trim().is_empty() {
        return String::new();
    }
    let n = match_ids.len();
    if n == 0 {
        i18n::chat_find_no_match(locale).to_string()
    } else {
        format!("{} / {}", cursor + 1, n)
    }
}

fn chat_find_nav_disabled(query: &str, match_ids: &[String]) -> bool {
    query.trim().is_empty() || match_ids.is_empty()
}

fn scroll_adjacent_find_match(
    ids: &[String],
    dir: ChatFindNavDir,
    chat_find_cursor: RwSignal<usize>,
    auto_scroll_chat: RwSignal<bool>,
) {
    if ids.is_empty() {
        return;
    }
    auto_scroll_chat.set(false);
    chat_find_cursor.update(|i| match dir {
        ChatFindNavDir::Prev => {
            if *i == 0 {
                *i = ids.len() - 1;
            } else {
                *i -= 1;
            }
        }
        ChatFindNavDir::Next => {
            *i = (*i + 1) % ids.len();
        }
    });
    let idx = chat_find_cursor.get();
    scroll_message_into_view(&ids[idx]);
}

#[component]
pub fn ChatFindBar(signals: ChatFindBarSignals) -> impl IntoView {
    let ChatFindBarSignals {
        chat_find_panel_open,
        locale,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        auto_scroll_chat,
    } = signals;
    view! {
        <div class="chat-find-wrap">
            <div class="chat-find-bar" role="search" prop:aria-label=move || i18n::chat_find_region(locale.get())>
                <label class="chat-find-label" for="chat-find-input">{move || i18n::chat_find_label(locale.get())}</label>
                <input
                    id="chat-find-input"
                    type="search"
                    class="chat-find-input"
                    prop:placeholder=move || i18n::chat_find_ph(locale.get())
                    prop:value=move || chat_find_query.get()
                    on:input=move |ev| {
                        chat_find_query.set(event_target_value(&ev));
                    }
                />
                <span class="chat-find-meta" aria-live="polite">
                    {move || {
                        chat_find_meta_line(
                            locale.get(),
                            &chat_find_query.get(),
                            &chat_find_match_ids.get(),
                            chat_find_cursor.get(),
                        )
                    }}
                </span>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-nav"
                    prop:title=move || i18n::chat_find_prev_title(locale.get())
                    prop:disabled=move || {
                        chat_find_nav_disabled(&chat_find_query.get(), &chat_find_match_ids.get())
                    }
                    on:click=move |_| {
                        let ids = chat_find_match_ids.get();
                        scroll_adjacent_find_match(
                            &ids,
                            ChatFindNavDir::Prev,
                            chat_find_cursor,
                            auto_scroll_chat,
                        );
                    }
                >
                    "↑"
                </button>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-nav"
                    prop:title=move || i18n::chat_find_next_title(locale.get())
                    prop:disabled=move || {
                        chat_find_nav_disabled(&chat_find_query.get(), &chat_find_match_ids.get())
                    }
                    on:click=move |_| {
                        let ids = chat_find_match_ids.get();
                        scroll_adjacent_find_match(
                            &ids,
                            ChatFindNavDir::Next,
                            chat_find_cursor,
                            auto_scroll_chat,
                        );
                    }
                >
                    "↓"
                </button>
                <button
                    type="button"
                    class="btn btn-muted btn-sm chat-find-close"
                    prop:title=move || i18n::chat_find_close_title(locale.get())
                    prop:aria-label=move || i18n::chat_find_close_aria(locale.get())
                    on:click=move |_| chat_find_panel_open.set(false)
                >
                    "×"
                </button>
            </div>
        </div>
    }
}
