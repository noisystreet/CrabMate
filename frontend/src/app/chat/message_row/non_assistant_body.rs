//! 非助手气泡正文（工具摘要、跳转用户消息、纯文本高亮）。

use std::collections::HashSet;

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::message_format::message_text_for_display_ex;
use crate::session_search::{normalize_search_query, split_for_find_highlight};
use crate::storage::{ChatSession, StoredMessage};

use super::super::message_row_actions::spawn_scroll_to_linked_user_message;
use super::helpers::{live_message_reasoning_text, tool_bubble_emoji};

fn render_highlighted_message_text(
    msg: &StoredMessage,
    loc: Locale,
    apply_filters: bool,
    query: &str,
) -> AnyView {
    let disp = message_text_for_display_ex(msg, loc, apply_filters);
    let segs = split_for_find_highlight(&disp, query);
    segs.into_iter()
        .map(|(s, hl)| {
            if hl {
                view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
            } else {
                view! { {s} }.into_any()
            }
        })
        .collect_view()
        .into_any()
}

fn highlighted_body_span(
    m_for_body: StoredMessage,
    locale: RwSignal<Locale>,
    chat_find_query: RwSignal<String>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    move || {
        let apply = apply_assistant_display_filters.get();
        let loc = locale.get();
        let q = normalize_search_query(&chat_find_query.get());
        render_highlighted_message_text(&m_for_body, loc, apply, &q)
    }
}

fn tool_compact_body_view(
    m_for_body: StoredMessage,
    detail_snapshot: Option<String>,
    reasoning_live: Option<(RwSignal<Vec<ChatSession>>, RwSignal<String>, String)>,
    tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    tool_mid: String,
    locale: RwSignal<Locale>,
    chat_find_query: RwSignal<String>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> AnyView {
    let tool_emoji = tool_bubble_emoji(&m_for_body);
    let mid_store = StoredValue::new(tool_mid);
    view! {
        <div class="msg-tool-compact">
            <Show when=move || {
                let live_ok = reasoning_live.as_ref().is_some_and(|(sess, aid, mid)| {
                    !live_message_reasoning_text(*sess, *aid, mid.as_str())
                        .trim()
                        .is_empty()
                });
                let snap_ok = detail_snapshot
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty());
                live_ok || snap_ok
            }>
                <button
                    type="button"
                    class="msg-tool-drawer-btn msg-tool-drawer-icon-btn"
                    prop:title=move || {
                        let mid = mid_store.get_value();
                        if tool_detail_expanded_ids.with(|s| s.contains(mid.as_str())) {
                            i18n::msg_tool_detail_collapse_title(locale.get())
                        } else {
                            i18n::msg_tool_detail_expand_title(locale.get())
                        }
                    }
                    prop:aria-label=move || {
                        let mid = mid_store.get_value();
                        if tool_detail_expanded_ids.with(|s| s.contains(mid.as_str())) {
                            i18n::msg_tool_detail_collapse_title(locale.get())
                        } else {
                            i18n::msg_tool_detail_expand_title(locale.get())
                        }
                    }
                    on:click=move |_| {
                        let id = mid_store.get_value().clone();
                        tool_detail_expanded_ids.update(|set| {
                            if set.contains(&id) {
                                set.remove(&id);
                            } else {
                                set.insert(id);
                            }
                        });
                    }
                >
                    <svg
                        class=move || {
                            let mid = mid_store.get_value();
                            if tool_detail_expanded_ids.with(|s| s.contains(mid.as_str())) {
                                "msg-tool-drawer-icon is-open"
                            } else {
                                "msg-tool-drawer-icon"
                            }
                        }
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <polyline points="6 9 12 15 18 9" />
                    </svg>
                </button>
            </Show>
            <span class="msg-tool-emoji" aria-hidden="true">{tool_emoji}</span>
            <span class="msg-body msg-tool-summary">
                {highlighted_body_span(
                    m_for_body.clone(),
                    locale,
                    chat_find_query,
                    apply_assistant_display_filters,
                )}
            </span>
        </div>
    }
    .into_any()
}

fn jump_uid_body_view(
    m_for_body: StoredMessage,
    uid: String,
    locale: RwSignal<Locale>,
    chat_find_query: RwSignal<String>,
    apply_assistant_display_filters: RwSignal<bool>,
    auto_scroll_chat: RwSignal<bool>,
) -> AnyView {
    let uid_click = uid.clone();
    let uid_key = uid;
    view! {
        <span
            class="msg-body msg-tool-body-jump"
            role="link"
            tabindex="0"
            prop:title=move || i18n::msg_jump_user_title(locale.get())
            prop:aria-label=move || i18n::msg_jump_user_aria(locale.get())
            on:click=move |_| {
                spawn_scroll_to_linked_user_message(&uid_click, auto_scroll_chat);
            }
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                let k = ev.key();
                if k == "Enter" || k == " " {
                    ev.prevent_default();
                    spawn_scroll_to_linked_user_message(&uid_key, auto_scroll_chat);
                }
            }
        >
            {highlighted_body_span(
                m_for_body,
                locale,
                chat_find_query,
                apply_assistant_display_filters,
            )}
        </span>
    }
    .into_any()
}

fn plain_highlight_body_view(
    m_for_body: StoredMessage,
    locale: RwSignal<Locale>,
    chat_find_query: RwSignal<String>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> AnyView {
    view! {
        <span class="msg-body">
            {highlighted_body_span(
                m_for_body,
                locale,
                chat_find_query,
                apply_assistant_display_filters,
            )}
        </span>
    }
    .into_any()
}

pub(super) struct NonAssistantMessageBodyParams {
    pub m_for_body: StoredMessage,
    pub is_tool_bubble: bool,
    pub tool_detail_text: Option<String>,
    /// 与 [`crate::app::chat::message_row::helpers::live_message_reasoning_text`] 对齐：工具气泡挂载时 `reasoning_text` 常为空，须订阅会话更新。
    pub tool_reasoning_live: Option<(RwSignal<Vec<ChatSession>>, RwSignal<String>, String)>,
    pub tool_detail_expanded_ids: RwSignal<HashSet<String>>,
    pub tool_mid: String,
    pub locale: RwSignal<Locale>,
    pub chat_find_query: RwSignal<String>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub jump_uid: Option<String>,
    pub auto_scroll_chat: RwSignal<bool>,
}

pub(super) fn build_non_assistant_message_body(p: NonAssistantMessageBodyParams) -> AnyView {
    let NonAssistantMessageBodyParams {
        m_for_body,
        is_tool_bubble,
        tool_detail_text,
        tool_reasoning_live,
        tool_detail_expanded_ids,
        tool_mid,
        locale,
        chat_find_query,
        apply_assistant_display_filters,
        jump_uid,
        auto_scroll_chat,
    } = p;
    if is_tool_bubble {
        return tool_compact_body_view(
            m_for_body,
            tool_detail_text,
            tool_reasoning_live,
            tool_detail_expanded_ids,
            tool_mid,
            locale,
            chat_find_query,
            apply_assistant_display_filters,
        );
    }
    if let Some(uid) = jump_uid {
        return jump_uid_body_view(
            m_for_body,
            uid,
            locale,
            chat_find_query,
            apply_assistant_display_filters,
            auto_scroll_chat,
        );
    }
    plain_highlight_body_view(
        m_for_body,
        locale,
        chat_find_query,
        apply_assistant_display_filters,
    )
}
