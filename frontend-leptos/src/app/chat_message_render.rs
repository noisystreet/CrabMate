//! 单条消息与「连续工具输出」分组的渲染（供 `chat_column` 使用）。

use std::collections::HashSet;

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::post_chat_branch;
use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::i18n::{self, Locale};
use crate::message_format::{is_staged_timeline_stored_message, message_text_for_display_ex};
use crate::session_ops::{
    format_msg_time_label, message_role_label, preceding_plain_user_message_id,
    truncate_at_user_message_and_prepare_regenerate, truncate_at_user_message_branch_local,
    user_ordinal_for_message_index, write_clipboard_text,
};
use crate::session_search::{
    normalize_search_query, scroll_message_into_view, split_for_find_highlight,
};
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, StoredMessage};

fn trigger_jump_to_user_prompt(uid: &str, auto_scroll_chat: RwSignal<bool>) {
    auto_scroll_chat.set(false);
    let u = uid.to_string();
    spawn_local(async move {
        TimeoutFuture::new(32).await;
        scroll_message_into_view(&u);
    });
}

pub(crate) enum ChatChunk {
    Single {
        idx: usize,
        msg: StoredMessage,
    },
    ToolGroup {
        head_id: String,
        items: Vec<(usize, StoredMessage)>,
    },
    StagedTimelineGroup {
        head_id: String,
        items: Vec<(usize, StoredMessage)>,
    },
}

pub(crate) fn chunk_messages(msgs: &[StoredMessage]) -> Vec<ChatChunk> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < msgs.len() {
        if msgs[i].is_tool {
            let start = i;
            while i < msgs.len() && msgs[i].is_tool {
                i += 1;
            }
            let slice: Vec<_> = (start..i).map(|j| (j, msgs[j].clone())).collect();
            if slice.len() == 1 {
                let (idx, msg) = slice.into_iter().next().expect("len 1");
                out.push(ChatChunk::Single { idx, msg });
            } else {
                let head_id = slice.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
                out.push(ChatChunk::ToolGroup {
                    head_id,
                    items: slice,
                });
            }
        } else if is_staged_timeline_stored_message(&msgs[i]) {
            let start = i;
            while i < msgs.len() && is_staged_timeline_stored_message(&msgs[i]) {
                i += 1;
            }
            let slice: Vec<_> = (start..i).map(|j| (j, msgs[j].clone())).collect();
            if slice.len() == 1 {
                let (idx, msg) = slice.into_iter().next().expect("len 1");
                out.push(ChatChunk::Single { idx, msg });
            } else {
                let head_id = slice.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
                out.push(ChatChunk::StagedTimelineGroup {
                    head_id,
                    items: slice,
                });
            }
        } else {
            out.push(ChatChunk::Single {
                idx: i,
                msg: msgs[i].clone(),
            });
            i += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tool_run_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    expanded_tool_run_heads: RwSignal<HashSet<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    status_busy: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    auto_scroll_chat: RwSignal<bool>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let items_sv = StoredValue::new(items);
    let group_ids: Vec<String> = items_sv
        .get_value()
        .iter()
        .map(|(_, m)| m.id.clone())
        .collect();
    let n = items_sv.get_value().len();
    let head_for_expand_hint = head_key.clone();
    let head_attr = head_key.clone();
    let fold_head = head_key.clone();
    view! {
        <div class="msg-tool-run" data-tool-run=head_attr>
            {move || {
                let expanded_known =
                    expanded_tool_run_heads.with(|s| s.contains(&fold_head));
                let find_hit = {
                    let q = normalize_search_query(&chat_find_query.get());
                    !q.is_empty()
                        && chat_find_match_ids.with(|ids| {
                            ids
                                .iter()
                                .any(|mid| group_ids.iter().any(|g| g == mid))
                        })
                };
                let show_all = expanded_known || find_hit;
                let entries: Vec<_> = items_sv.get_value();
                let fold_on_click = fold_head.clone();
                let expand_on_click = head_for_expand_hint.clone();
                if show_all {
                    view! {
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_collapse_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_collapse_aria(locale.get())
                                on:click=move |_| {
                                    let k = fold_on_click.clone();
                                    expanded_tool_run_heads.update(|s| {
                                        s.remove(&k);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_collapse_btn(locale.get())}
                            </button>
                        </div>
                        {
                            entries
                                .into_iter()
                                .map(|(msg_idx, m)| {
                                    chat_message_row(
                                        msg_idx,
                                        m,
                                        sessions,
                                        active_id,
                                        expanded_long_assistant_ids,
                                        chat_find_query,
                                        chat_find_match_ids,
                                        chat_find_cursor,
                                        bubble_md_select_mode,
                                        bubble_md_selected_ids,
                                        auto_scroll_chat,
                                        status_busy,
                                        session_sync,
                                        regen_stream_after_truncate,
                                        retry_assistant_target,
                                        status_err,
                                        locale,
                                        markdown_render,
                                        apply_assistant_display_filters,
                                    )
                                })
                                .collect_view()
                        }
                    }
                    .into_any()
                } else if let Some((msg_idx, last)) = entries.last().cloned() {
                    view! {
                        <div class="msg-tool-run-head" role="group" prop:aria-label=move || i18n::msg_tool_run_group_aria(locale.get())>
                            <span class="msg-tool-run-count">{move || i18n::msg_tool_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-tool-run-toggle"
                                prop:title=move || i18n::msg_tool_expand_title(locale.get())
                                prop:aria-label=move || i18n::msg_tool_expand_aria(locale.get())
                                on:click=move |_| {
                                    let h = expand_on_click.clone();
                                    expanded_tool_run_heads.update(|s| {
                                        s.insert(h);
                                    });
                                }
                            >
                                {move || i18n::msg_tool_expand_btn(locale.get())}
                            </button>
                        </div>
                        {chat_message_row(
                            msg_idx,
                            last,
                            sessions,
                            active_id,
                            expanded_long_assistant_ids,
                            chat_find_query,
                            chat_find_match_ids,
                            chat_find_cursor,
                            bubble_md_select_mode,
                            bubble_md_selected_ids,
                            auto_scroll_chat,
                            status_busy,
                            session_sync,
                            regen_stream_after_truncate,
                            retry_assistant_target,
                            status_err,
                            locale,
                            markdown_render,
                            apply_assistant_display_filters,
                        )}
                    }
                    .into_any()
                } else {
                    view! { <div class="msg-tool-run-empty"></div> }.into_any()
                }
            }}
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn staged_timeline_group_view(
    head_key: String,
    items: Vec<(usize, StoredMessage)>,
    expanded_staged_timeline_heads: RwSignal<HashSet<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    status_busy: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    auto_scroll_chat: RwSignal<bool>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let items_sv = StoredValue::new(items);
    let group_ids: Vec<String> = items_sv
        .get_value()
        .iter()
        .map(|(_, m)| m.id.clone())
        .collect();
    let n = items_sv.get_value().len();
    let head_for_expand_hint = head_key.clone();
    let head_attr = head_key.clone();
    let fold_head = head_key.clone();
    view! {
        <div class="msg-staged-timeline-run" data-staged-timeline-run=head_attr>
            {move || {
                let expanded_known =
                    expanded_staged_timeline_heads.with(|s| s.contains(&fold_head));
                let find_hit = {
                    let q = normalize_search_query(&chat_find_query.get());
                    !q.is_empty()
                        && chat_find_match_ids.with(|ids| {
                            ids
                                .iter()
                                .any(|mid| group_ids.iter().any(|g| g == mid))
                        })
                };
                let show_all = expanded_known || find_hit;
                let entries: Vec<_> = items_sv.get_value();
                let fold_on_click = fold_head.clone();
                let expand_on_click = head_for_expand_hint.clone();
                if show_all {
                    view! {
                        <div class="msg-staged-timeline-run-head" role="group" prop:aria-label=move || i18n::msg_staged_timeline_run_group_aria(locale.get())>
                            <span class="msg-staged-timeline-run-count">{move || i18n::msg_staged_timeline_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-staged-timeline-run-toggle"
                                prop:title=move || i18n::msg_staged_timeline_collapse_title(locale.get())
                                prop:aria-label=move || i18n::msg_staged_timeline_collapse_aria(locale.get())
                                on:click=move |_| {
                                    let k = fold_on_click.clone();
                                    expanded_staged_timeline_heads.update(|s| {
                                        s.remove(&k);
                                    });
                                }
                            >
                                {move || i18n::msg_staged_timeline_collapse_btn(locale.get())}
                            </button>
                        </div>
                        {
                            entries
                                .into_iter()
                                .map(|(msg_idx, m)| {
                                    chat_message_row(
                                        msg_idx,
                                        m,
                                        sessions,
                                        active_id,
                                        expanded_long_assistant_ids,
                                        chat_find_query,
                                        chat_find_match_ids,
                                        chat_find_cursor,
                                        bubble_md_select_mode,
                                        bubble_md_selected_ids,
                                        auto_scroll_chat,
                                        status_busy,
                                        session_sync,
                                        regen_stream_after_truncate,
                                        retry_assistant_target,
                                        status_err,
                                        locale,
                                        markdown_render,
                                        apply_assistant_display_filters,
                                    )
                                })
                                .collect_view()
                        }
                    }
                    .into_any()
                } else if let Some((msg_idx, last)) = entries.last().cloned() {
                    view! {
                        <div class="msg-staged-timeline-run-head" role="group" prop:aria-label=move || i18n::msg_staged_timeline_run_group_aria(locale.get())>
                            <span class="msg-staged-timeline-run-count">{move || i18n::msg_staged_timeline_run_count(locale.get(), n)}</span>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm msg-staged-timeline-run-toggle"
                                prop:title=move || i18n::msg_staged_timeline_expand_title(locale.get())
                                prop:aria-label=move || i18n::msg_staged_timeline_expand_aria(locale.get())
                                on:click=move |_| {
                                    let h = expand_on_click.clone();
                                    expanded_staged_timeline_heads.update(|s| {
                                        s.insert(h);
                                    });
                                }
                            >
                                {move || i18n::msg_staged_timeline_expand_btn(locale.get())}
                            </button>
                        </div>
                        {chat_message_row(
                            msg_idx,
                            last,
                            sessions,
                            active_id,
                            expanded_long_assistant_ids,
                            chat_find_query,
                            chat_find_match_ids,
                            chat_find_cursor,
                            bubble_md_select_mode,
                            bubble_md_selected_ids,
                            auto_scroll_chat,
                            status_busy,
                            session_sync,
                            regen_stream_after_truncate,
                            retry_assistant_target,
                            status_err,
                            locale,
                            markdown_render,
                            apply_assistant_display_filters,
                        )}
                    }
                    .into_any()
                } else {
                    view! { <div class="msg-staged-timeline-run-empty"></div> }.into_any()
                }
            }}
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn chat_message_row(
    msg_idx: usize,
    m: StoredMessage,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    auto_scroll_chat: RwSignal<bool>,
    status_busy: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let is_staged_timeline = is_staged_timeline_stored_message(&m);
    let cls = if is_staged_timeline {
        "msg msg-system msg-staged-timeline"
    } else {
        match m.role.as_str() {
            "user" => "msg msg-user",
            "assistant" if m.is_tool => "msg msg-tool",
            "assistant" => "msg msg-assistant",
            _ if m.is_tool => "msg msg-tool",
            _ => "msg msg-system",
        }
    };
    let loading = (m.role == "assistant" && m.state.as_deref() == Some("loading"))
        || (m.is_tool && m.state.as_deref() == Some("loading"));
    let err = m.state.as_deref() == Some("error");
    let class_prefix = if err {
        format!("{cls} msg-error")
    } else if loading {
        format!("{cls} msg-loading")
    } else {
        cls.to_string()
    };
    let mid_highlight = m.id.clone();
    let m_role = m.clone();
    let role_lbl = move || message_role_label(&m_role, locale.get());
    let time_str = format_msg_time_label(m.created_at).unwrap_or_default();
    let mid_retry = m.id.clone();
    let copy_id = m.id.clone();
    let user_retry_id = m.id.clone();
    let user_branch_id = m.id.clone();
    let is_user_plain = m.role == "user" && !m.is_tool;
    let is_tool_bubble = m.is_tool;
    let jump_uid = if is_tool_bubble {
        sessions.with(|list| {
            let aid = active_id.get();
            list.iter()
                .find(|s| s.id == aid)
                .and_then(|sess| preceding_plain_user_message_id(&sess.messages, msg_idx))
        })
    } else {
        None
    };
    let show_msg_action_bar = !is_tool_bubble || is_user_plain || err;
    let msg_core = if m.role == "assistant" && !m.is_tool {
        assistant_markdown_collapsible_view(
            sessions,
            active_id,
            m.id.clone(),
            expanded_long_assistant_ids,
            locale,
            markdown_render,
            apply_assistant_display_filters,
        )
        .into_any()
    } else {
        let m_for_body = m.clone();
        let asc = auto_scroll_chat;
        let body_inner = match jump_uid {
            Some(uid) => {
                let uid_click = uid.clone();
                let uid_key = uid.clone();
                view! {
                    <span
                        class="msg-body msg-tool-body-jump"
                        role="link"
                        tabindex="0"
                        prop:title=move || i18n::msg_jump_user_title(locale.get())
                        prop:aria-label=move || i18n::msg_jump_user_aria(locale.get())
                        on:click=move |_| {
                            trigger_jump_to_user_prompt(&uid_click, asc);
                        }
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            let k = ev.key();
                            if k == "Enter" || k == " " {
                                ev.prevent_default();
                                trigger_jump_to_user_prompt(&uid_key, asc);
                            }
                        }
                    >
                        {move || {
                            let apply = apply_assistant_display_filters.get();
                            let loc = locale.get();
                            let q = normalize_search_query(&chat_find_query.get());
                            let disp =
                                message_text_for_display_ex(&m_for_body, loc, apply);
                            let segs = split_for_find_highlight(&disp, &q);
                            segs
                                .into_iter()
                                .map(|(s, hl)| {
                                    if hl {
                                        view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
                                    } else {
                                        view! { {s} }.into_any()
                                    }
                                })
                                .collect_view()
                        }}
                    </span>
                }
                .into_any()
            }
            None => view! {
                <span class="msg-body">
                    {move || {
                        let apply = apply_assistant_display_filters.get();
                        let loc = locale.get();
                        let q = normalize_search_query(&chat_find_query.get());
                        let disp =
                            message_text_for_display_ex(&m_for_body, loc, apply);
                        let segs = split_for_find_highlight(&disp, &q);
                        segs
                            .into_iter()
                            .map(|(s, hl)| {
                                if hl {
                                    view! { <mark class="msg-find-inline">{s}</mark> }.into_any()
                                } else {
                                    view! { {s} }.into_any()
                                }
                            })
                            .collect_view()
                    }}
                </span>
            }
            .into_any(),
        };
        if is_user_plain && !m.image_urls.is_empty() {
            let imgs: Vec<String> = m.image_urls.clone();
            view! {
                <div class="msg-user-with-images">
                    <div class="msg-user-images">
                        {imgs
                            .into_iter()
                            .map(|u| {
                                view! { <img class="msg-user-img" src=u alt="" /> }.into_any()
                            })
                            .collect_view()}
                    </div>
                    {body_inner}
                </div>
            }
            .into_any()
        } else {
            body_inner
        }
    };
    let mid_for_select = StoredValue::new(m.id.clone());
    let mid_dom = m.id.clone();
    view! {
        <div class="msg-with-select">
            <Show when=move || bubble_md_select_mode.get()>
                <label class="msg-select-label" prop:title=move || i18n::msg_select_label_title(locale.get())>
                    <input
                        type="checkbox"
                        class="msg-select-cb"
                        prop:aria-label=move || i18n::msg_select_cb_aria(locale.get())
                        prop:checked=move || {
                            let mid = mid_for_select.get_value();
                            bubble_md_selected_ids.with(|v| v.contains(&mid))
                        }
                        on:change=move |_| {
                            let mid = mid_for_select.get_value();
                            bubble_md_selected_ids.update(|v| {
                                if let Some(i) = v.iter().position(|x| x == &mid) {
                                    v.remove(i);
                                } else {
                                    v.push(mid);
                                }
                            });
                        }
                    />
                </label>
            </Show>
            <div class="msg-stack">
                <div
                    class=move || {
                        let mut c = class_prefix.clone();
                        let q = normalize_search_query(&chat_find_query.get());
                        if !q.is_empty() {
                            let in_list = chat_find_match_ids.with(|ids| {
                                ids.iter().any(|x| x == &mid_highlight)
                            });
                            if in_list {
                                c.push_str(" msg-find-match");
                            }
                            let cur = chat_find_cursor.get();
                            let is_current = chat_find_match_ids.with(|ids| {
                                ids
                                    .get(cur)
                                    .map(|x| x == &mid_highlight)
                                    .unwrap_or(false)
                            });
                            if is_current {
                                c.push_str(" msg-find-highlight");
                            }
                        }
                        c
                    }
                    id=format!("msg-{mid_dom}")
                    data-testid=move || {
                        if is_tool_bubble {
                            "chat-tool-card"
                        } else {
                            "chat-message-row"
                        }
                    }
                >
                    <div class="msg-meta" aria-hidden="true">
                        <span class="msg-meta-role">{role_lbl}</span>
                        <span class="msg-meta-time">{time_str}</span>
                    </div>
                    {msg_core}
                    {loading.then(|| {
                        view! {
                            <span class="typing-dots" aria-hidden="true">
                                <span></span>
                                <span></span>
                                <span></span>
                            </span>
                        }
                    })}
                </div>
                {show_msg_action_bar.then(|| {
                    view! {
                        <div class="msg-actions msg-actions-below" role="group" prop:aria-label=move || i18n::msg_actions_group_aria(locale.get())>
                            {(!is_tool_bubble).then(|| {
                                view! {
                                    <button
                                        type="button"
                                        class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                                        prop:title=move || i18n::msg_copy_title(locale.get())
                                        prop:aria-label=move || i18n::msg_copy_aria(locale.get())
                                        on:click=move |_| {
                                            let loc = locale.get_untracked();
                                            let apply =
                                                apply_assistant_display_filters.get_untracked();
                                            let t = sessions.with(|list| {
                                                let aid = active_id.get_untracked();
                                                list.iter()
                                                    .find(|s| s.id == aid)
                                                    .and_then(|s| {
                                                        s.messages
                                                            .iter()
                                                            .find(|msg| msg.id == copy_id)
                                                    })
                                                    .map(|msg| {
                                                        message_text_for_display_ex(msg, loc, apply)
                                                    })
                                                    .unwrap_or_default()
                                            });
                                            write_clipboard_text(&t, loc);
                                        }
                                    >
                                        <svg
                                            class="msg-action-icon"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            xmlns="http://www.w3.org/2000/svg"
                                            aria-hidden="true"
                                        >
                                            <rect
                                                x="9"
                                                y="9"
                                                width="13"
                                                height="13"
                                                rx="2"
                                                stroke="currentColor"
                                                stroke-width="2"
                                            />
                                            <path
                                                d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"
                                                stroke="currentColor"
                                                stroke-width="2"
                                            />
                                        </svg>
                                    </button>
                                }
                            })}
                            {is_user_plain.then(|| {
                                let idx = msg_idx;
                                let uid_r = user_retry_id.clone();
                                let uid_b = user_branch_id.clone();
                                view! {
                                    <button
                                        type="button"
                                        class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                                        prop:title=move || i18n::msg_regen_title(locale.get())
                                        prop:aria-label=move || i18n::msg_regen_aria(locale.get())
                                        prop:disabled=move || status_busy.get()
                                        on:click=move |_| {
                                            if status_busy.get() {
                                                return;
                                            }
                                            let (cid, rev) = session_sync.with(|s| {
                                                let (a, b) = s.branch_id_and_expected_revision();
                                                (a.map(|x| x.to_string()), b)
                                            });
                                            let ord = sessions.with(|list| {
                                                let aid = active_id.get_untracked();
                                                list.iter()
                                                    .find(|s| s.id == aid)
                                                    .and_then(|s| {
                                                        user_ordinal_for_message_index(
                                                            &s.messages,
                                                            idx,
                                                        )
                                                    })
                                            });
                                            let uid = uid_r.clone();
                                            match (cid, rev, ord) {
                                                (
                                                    Some(conv),
                                                    Some(exp_rev),
                                                    Some(before_ord),
                                                ) => {
                                                    let loc = locale.get_untracked();
                                                    spawn_local(async move {
                                                        match post_chat_branch(
                                                            &conv,
                                                            before_ord,
                                                            exp_rev,
                                                            loc,
                                                        )
                                                        .await
                                                        {
                                                            Ok(new_rev) => {
                                                                session_sync.update(|s| {
                                                                    s.set_revision_after_branch(
                                                                        new_rev,
                                                                    );
                                                                });
                                                                let mut prep: Option<
                                                                    (String, Vec<String>, String),
                                                                > = None;
                                                                sessions.update(|list| {
                                                                    let aid = active_id
                                                                        .get_untracked();
                                                                    prep = truncate_at_user_message_and_prepare_regenerate(
                                                                        list,
                                                                        &aid,
                                                                        &uid,
                                                                    );
                                                                });
                                                                if let Some((ut, uimg, aid)) = prep {
                                                                    regen_stream_after_truncate
                                                                        .set(Some((ut, uimg, aid)));
                                                                }
                                                            }
                                                            Err(e) => {
                                                                session_sync
                                                                    .update(|s| s.mark_branch_conflict());
                                                                status_err.set(Some(e));
                                                            }
                                                        }
                                                    });
                                                }
                                                _ => {
                                                    let mut prep: Option<(String, Vec<String>, String)> = None;
                                                    sessions.update(|list| {
                                                        let aid = active_id.get_untracked();
                                                        prep = truncate_at_user_message_and_prepare_regenerate(
                                                            list,
                                                            &aid,
                                                            &uid,
                                                        );
                                                    });
                                                    if let Some((ut, uimg, aid)) = prep {
                                                        regen_stream_after_truncate
                                                            .set(Some((ut, uimg, aid)));
                                                    }
                                                }
                                            }
                                        }
                                    >
                                        <svg
                                            class="msg-action-icon"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            xmlns="http://www.w3.org/2000/svg"
                                            aria-hidden="true"
                                        >
                                            <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                                            <path d="M21 3v5h-5" />
                                            <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                                            <path d="M8 16H3v5" />
                                        </svg>
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                                        prop:title=move || i18n::msg_branch_title(locale.get())
                                        prop:aria-label=move || i18n::msg_branch_aria(locale.get())
                                        prop:disabled=move || status_busy.get()
                                        on:click=move |_| {
                                            if status_busy.get() {
                                                return;
                                            }
                                            let (cid, rev) = session_sync.with(|s| {
                                                let (a, b) = s.branch_id_and_expected_revision();
                                                (a.map(|x| x.to_string()), b)
                                            });
                                            let ord = sessions.with(|list| {
                                                let aid = active_id.get_untracked();
                                                list.iter()
                                                    .find(|s| s.id == aid)
                                                    .and_then(|s| {
                                                        user_ordinal_for_message_index(
                                                            &s.messages,
                                                            idx,
                                                        )
                                                    })
                                            });
                                            let uid = uid_b.clone();
                                            match (cid, rev, ord) {
                                                (
                                                    Some(conv),
                                                    Some(exp_rev),
                                                    Some(before_ord),
                                                ) => {
                                                    let loc_b = locale.get_untracked();
                                                    spawn_local(async move {
                                                        match post_chat_branch(
                                                            &conv,
                                                            before_ord,
                                                            exp_rev,
                                                            loc_b,
                                                        )
                                                        .await
                                                        {
                                                            Ok(new_rev) => {
                                                                session_sync.update(|s| {
                                                                    s.set_revision_after_branch(
                                                                        new_rev,
                                                                    );
                                                                });
                                                                sessions.update(|list| {
                                                                    let aid = active_id
                                                                        .get_untracked();
                                                                    let _ = truncate_at_user_message_branch_local(
                                                                        list,
                                                                        &aid,
                                                                        &uid,
                                                                    );
                                                                });
                                                            }
                                                            Err(e) => {
                                                                session_sync
                                                                    .update(|s| s.mark_branch_conflict());
                                                                status_err.set(Some(e));
                                                            }
                                                        }
                                                    });
                                                }
                                                _ => {
                                                    sessions.update(|list| {
                                                        let aid = active_id.get_untracked();
                                                        let _ = truncate_at_user_message_branch_local(
                                                            list,
                                                            &aid,
                                                            &uid,
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                    >
                                        <svg
                                            class="msg-action-icon"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            xmlns="http://www.w3.org/2000/svg"
                                            aria-hidden="true"
                                        >
                                            <line
                                                x1="6"
                                                y1="3"
                                                x2="6"
                                                y2="15"
                                                fill="none"
                                            />
                                            <circle cx="6" cy="3" r="2" fill="none" />
                                            <path
                                                d="M6 15v-1a4 4 0 0 1 4-4h4a4 4 0 0 0 4-4V5"
                                                fill="none"
                                            />
                                            <circle cx="18" cy="5" r="2" fill="none" />
                                            <circle cx="18" cy="19" r="2" fill="none" />
                                            <path d="M18 7v12" fill="none" />
                                        </svg>
                                    </button>
                                }
                            })}
                            {err.then(move || {
                                let mid = mid_retry.clone();
                                view! {
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm msg-action-icon-btn"
                                        prop:title=move || i18n::msg_retry_title(locale.get())
                                        prop:aria-label=move || i18n::msg_retry_aria(locale.get())
                                        prop:disabled=move || status_busy.get()
                                        on:click=move |_| {
                                            retry_assistant_target.set(Some(mid.clone()));
                                        }
                                    >
                                        <svg
                                            class="msg-action-icon"
                                            viewBox="0 0 24 24"
                                            fill="none"
                                            stroke="currentColor"
                                            stroke-width="2"
                                            stroke-linecap="round"
                                            stroke-linejoin="round"
                                            xmlns="http://www.w3.org/2000/svg"
                                            aria-hidden="true"
                                        >
                                            <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
                                            <path d="M21 3v5h-5" />
                                            <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
                                            <path d="M8 16H3v5" />
                                        </svg>
                                    </button>
                                }
                            })}
                        </div>
                    }
                })}
            </div>
        </div>
    }
}
