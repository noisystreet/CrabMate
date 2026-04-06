//! 中部聊天列：消息列表、输入框、查找/多选入口。

use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use super::scroll_guard::MessagesScrollFromEffectGuard;
use crate::api::post_chat_branch;
use crate::app_prefs::AUTO_SCROLL_RESUME_GAP_PX;
use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::message_format::message_text_for_display;
use crate::session_ops::{
    clamp_session_ctx_menu_pos, format_msg_time_label, message_role_label,
    truncate_at_user_message_and_prepare_regenerate, truncate_at_user_message_branch_local,
    user_ordinal_for_message_index, write_clipboard_text,
};
use crate::session_search::{normalize_search_query, split_for_find_highlight};
use crate::storage::ChatSession;

#[allow(clippy::too_many_arguments)]
pub fn chat_column_view(
    messages_scroller: NodeRef<leptos::html::Div>,
    auto_scroll_chat: RwSignal<bool>,
    messages_scroll_from_effect: RwSignal<bool>,
    last_messages_scroll_top: RwSignal<i32>,
    session_context_menu: RwSignal<Option<crate::session_ops::SessionContextAnchor>>,
    chat_export_ctx_menu: RwSignal<Option<(f64, f64)>>,
    chat_find_panel_open: RwSignal<bool>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    composer_input_ref: NodeRef<Textarea>,
    composer_buf_ta: Arc<Mutex<String>>,
    run_send_message: Arc<dyn Fn() + Send + Sync>,
    trigger_stop: Arc<dyn Fn() + Send + Sync>,
    status_busy: RwSignal<bool>,
    initialized: RwSignal<bool>,
    conversation_id: RwSignal<Option<String>>,
    conversation_revision: RwSignal<Option<u64>>,
    regen_stream_after_truncate: RwSignal<Option<(String, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
                <div
                    class="chat-column"
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() != "End" {
                            return;
                        }
                        let Some(t) = ev.target() else {
                            return;
                        };
                        let Ok(he) = t.dyn_into::<web_sys::HtmlElement>() else {
                            return;
                        };
                        let tag = he.tag_name();
                        if tag.eq_ignore_ascii_case("TEXTAREA")
                            || tag.eq_ignore_ascii_case("INPUT")
                            || tag.eq_ignore_ascii_case("SELECT")
                            || tag.eq_ignore_ascii_case("OPTION")
                        {
                            return;
                        }
                        if he.is_content_editable() {
                            return;
                        }
                        ev.prevent_default();
                        auto_scroll_chat.set(true);
                        let mref = messages_scroller;
                        let scroll_from_effect = messages_scroll_from_effect;
                        spawn_local(async move {
                            let _guard = MessagesScrollFromEffectGuard::new(scroll_from_effect);
                            TimeoutFuture::new(0).await;
                            if let Some(el) = mref.get() {
                                el.set_scroll_top(el.scroll_height());
                            }
                            TimeoutFuture::new(0).await;
                            if let Some(el) = mref.get() {
                                el.set_scroll_top(el.scroll_height());
                            }
                            TimeoutFuture::new(16).await;
                            if let Some(el) = mref.get() {
                                el.set_scroll_top(el.scroll_height());
                            }
                        });
                    }
                >
                    <Show when=move || !chat_find_panel_open.get()>
                        <button
                            type="button"
                            class="bubble-md-toggle"
                            title="多选消息导出 Markdown（聊天区亦可右键）"
                            aria-label="多选导出 Markdown"
                            aria-pressed=move || bubble_md_select_mode.get()
                            on:click=move |_| {
                                let next = !bubble_md_select_mode.get();
                                bubble_md_select_mode.set(next);
                                if !next {
                                    bubble_md_selected_ids.set(Vec::new());
                                }
                            }
                        >
                            <svg
                                class="bubble-md-toggle-icon"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <path d="M9 11l2 2 4-4" />
                                <path d="M4 6h.01" />
                                <path d="M4 12h.01" />
                                <path d="M4 18h.01" />
                                <path d="M8 6h13" />
                                <path d="M8 12h13" />
                                <path d="M8 18h9" />
                            </svg>
                        </button>
                    </Show>
                    <Show when=move || !chat_find_panel_open.get()>
                        <button
                            type="button"
                            class="chat-find-toggle"
                            title="在当前会话中查找"
                            aria-label="在当前会话中查找"
                            aria-expanded="false"
                            on:click=move |_| chat_find_panel_open.set(true)
                        >
                            <svg
                                class="chat-find-icon"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <circle cx="11" cy="11" r="8" />
                                <path d="m21 21-4.3-4.3" />
                            </svg>
                        </button>
                    </Show>
                    <div
                        class="messages"
                        node_ref=messages_scroller
                        on:contextmenu=move |ev: web_sys::MouseEvent| {
                            ev.prevent_default();
                            session_context_menu.set(None);
                            let (x, y) = clamp_session_ctx_menu_pos(ev.client_x(), ev.client_y());
                            chat_export_ctx_menu.set(Some((x, y)));
                        }
                        on:wheel=move |ev: web_sys::WheelEvent| {
                            // 用户上滚查看历史时，立即关闭自动跟底，避免流式期间被强行拉回底部。
                            if ev.delta_y() < 0.0 {
                                auto_scroll_chat.set(false);
                            }
                        }
                        on:scroll=move |ev: web_sys::Event| {
                            if let Some(t) = ev.target() {
                                if let Ok(el) = t.dyn_into::<web_sys::HtmlElement>() {
                                    let top = el.scroll_top();
                                    let prev_top = last_messages_scroll_top.get_untracked();
                                    last_messages_scroll_top.set(top);
                                    if messages_scroll_from_effect.get_untracked() {
                                        return;
                                    }
                                    let gap = el.scroll_height()
                                        - top
                                        - el.client_height();
                                    if gap > AUTO_SCROLL_RESUME_GAP_PX {
                                        auto_scroll_chat.set(false);
                                    } else if !auto_scroll_chat.get_untracked() && top >= prev_top {
                                        // 仅在向下滚且回到底部附近时恢复自动跟底。
                                        auto_scroll_chat.set(true);
                                    }
                                }
                            }
                        }
                    >
                        <div class="chat-thread">
                        <div class="messages-inner">
                            {move || {
                                let id = active_id.get();
                                sessions.with(|list| {
                                    let msgs = list
                                        .iter()
                                        .find(|s| s.id == id)
                                        .map(|s| s.messages.clone())
                                        .unwrap_or_default();
                                    if msgs.is_empty() {
                                        view! {
                                            <div class="messages-empty" role="status">
                                                <div class="messages-empty-card">
                                                    <p class="messages-empty-title">"开始对话"</p>
                                                    <p class="messages-empty-lead">
                                                        "在下方输入消息，Enter 发送，Shift+Enter 换行。"
                                                    </p>
                                                    <ul class="messages-empty-tips">
                                                        <li>"左侧可新建对话、切换最近会话，或「管理会话」导出与重命名。"</li>
                                                        <li>"侧栏展开时工具栏在右列顶部；「隐藏侧栏」后右侧贴边纵向三键，同宽铺满一条，无额外围框。视图菜单可在隐藏、工作区、任务之间切换。"</li>
                                                    </ul>
                                                </div>
                                            </div>
                                        }
                                        .into_any()
                                    } else {
                                        msgs
                                            .into_iter()
                                            .enumerate()
                                            .map(|(msg_idx, m)| {
                                                let cls = match m.role.as_str() {
                                                    "user" => "msg msg-user",
                                                    "assistant" if m.is_tool => "msg msg-tool",
                                                    "assistant" => "msg msg-assistant",
                                                    _ if m.is_tool => "msg msg-tool",
                                                    _ => "msg msg-system",
                                                };
                                                let loading = m.role == "assistant"
                                                    && m.state.as_deref() == Some("loading");
                                                let err = m.state.as_deref() == Some("error");
                                                let class_prefix = if err {
                                                    format!("{cls} msg-error")
                                                } else if loading {
                                                    format!("{cls} msg-loading")
                                                } else {
                                                    cls.to_string()
                                                };
                                                let mid_highlight = m.id.clone();
                                                let role_lbl = message_role_label(&m);
                                                let time_str =
                                                    format_msg_time_label(m.created_at).unwrap_or_default();
                                                let mid_retry = m.id.clone();
                                                let copy_id = m.id.clone();
                                                let user_retry_id = m.id.clone();
                                                let user_branch_id = m.id.clone();
                                                let is_user_plain = m.role == "user" && !m.is_tool;
                                                let is_tool_bubble = m.is_tool;
                                                let show_msg_action_bar =
                                                    !is_tool_bubble || is_user_plain || err;
                                                let msg_core = if m.role == "assistant" && !m.is_tool {
                                                    assistant_markdown_collapsible_view(
                                                        sessions,
                                                        active_id,
                                                        m.id.clone(),
                                                        expanded_long_assistant_ids,
                                                    )
                                                    .into_any()
                                                } else {
                                                    let display_for_find = message_text_for_display(&m);
                                                    view! {
                                                        <span class="msg-body">
                                                            {move || {
                                                                let q =
                                                                    normalize_search_query(&chat_find_query.get());
                                                                let segs =
                                                                    split_for_find_highlight(&display_for_find, &q);
                                                                segs
                                                                    .into_iter()
                                                                    .map(|(s, hl)| {
                                                                        if hl {
                                                                            view! { <mark class="msg-find-inline">{s}</mark> }
                                                                                .into_any()
                                                                        } else {
                                                                            view! { {s} }.into_any()
                                                                        }
                                                                    })
                                                                    .collect_view()
                                                            }}
                                                        </span>
                                                    }
                                                    .into_any()
                                                };
                                                let mid_for_select = StoredValue::new(m.id.clone());
                                                view! {
                                                    <div class="msg-with-select">
                                                    <Show when=move || bubble_md_select_mode.get()>
                                                        <label class="msg-select-label" title="选中以加入导出">
                                                            <input
                                                                type="checkbox"
                                                                class="msg-select-cb"
                                                                aria-label="选中此条以导出 Markdown"
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
                                                        id=format!("msg-{}", m.id)
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
                                                    <div class="msg-actions msg-actions-below" role="group" aria-label="消息操作">
                                                            {(!is_tool_bubble).then(|| {
                                                                view! {
                                                            <button
                                                                type="button"
                                                                class="btn btn-muted btn-sm msg-action-btn msg-action-icon-btn"
                                                                title="复制本条展示文本"
                                                                aria-label="复制本条展示文本"
                                                                on:click=move |_| {
                                                                    let t = sessions.with(|list| {
                                                                        let aid = active_id.get_untracked();
                                                                        list.iter()
                                                                            .find(|s| s.id == aid)
                                                                            .and_then(|s| {
                                                                                s.messages
                                                                                    .iter()
                                                                                    .find(|msg| msg.id == copy_id)
                                                                            })
                                                                            .map(message_text_for_display)
                                                                            .unwrap_or_default()
                                                                    });
                                                                    write_clipboard_text(&t);
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
                                                                        title="删除本条及之后消息并重新生成（服务端会话需已持久化）"
                                                                        aria-label="从此处重试"
                                                                        prop:disabled=move || status_busy.get()
                                                                        on:click=move |_| {
                                                                            if status_busy.get() {
                                                                                return;
                                                                            }
                                                                            let cid = conversation_id.get();
                                                                            let rev = conversation_revision.get();
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
                                                                                    spawn_local(async move {
                                                                                        match post_chat_branch(
                                                                                            &conv,
                                                                                            before_ord,
                                                                                            exp_rev,
                                                                                        )
                                                                                        .await
                                                                                        {
                                                                                            Ok(new_rev) => {
                                                                                                conversation_revision
                                                                                                    .set(Some(new_rev));
                                                                                                let mut prep: Option<
                                                                                                    (String, String),
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
                                                                                                if let Some((ut, aid)) =
                                                                                                    prep
                                                                                                {
                                                                                                    regen_stream_after_truncate
                                                                                                        .set(Some((ut, aid)));
                                                                                                }
                                                                                            }
                                                                                            Err(e) => {
                                                                                                status_err.set(Some(e));
                                                                                            }
                                                                                        }
                                                                                    });
                                                                                }
                                                                                _ => {
                                                                                    let mut prep: Option<
                                                                                        (String, String),
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
                                                                                    if let Some((ut, aid)) = prep {
                                                                                        regen_stream_after_truncate
                                                                                            .set(Some((ut, aid)));
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
                                                                        title="删除本条及之后消息（不自动发送；服务端会话同步截断需已持久化）"
                                                                        aria-label="分支对话"
                                                                        prop:disabled=move || status_busy.get()
                                                                        on:click=move |_| {
                                                                            if status_busy.get() {
                                                                                return;
                                                                            }
                                                                            let cid = conversation_id.get();
                                                                            let rev = conversation_revision.get();
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
                                                                                    spawn_local(async move {
                                                                                        match post_chat_branch(
                                                                                            &conv,
                                                                                            before_ord,
                                                                                            exp_rev,
                                                                                        )
                                                                                        .await
                                                                                        {
                                                                                            Ok(new_rev) => {
                                                                                                conversation_revision
                                                                                                    .set(Some(new_rev));
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
                                                                                                status_err.set(Some(e));
                                                                                            }
                                                                                        }
                                                                                    });
                                                                                }
                                                                                _ => {
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
                                                                        title="重试当前助手生成"
                                                                        aria-label="重试"
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
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                })
                            }}
                        </div>
                        </div>
                    </div>
                    <div class="composer composer-ds">
                        <div class="composer-inner-ds">
                        <textarea
                            class="composer-input"
                            node_ref=composer_input_ref
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                *composer_buf_ta.lock().unwrap() = v;
                            }
                            on:keydown={
                                let r = Arc::clone(&run_send_message);
                                move |ev: web_sys::KeyboardEvent| {
                                    if ev.key() == "Enter" && !ev.shift_key() {
                                        ev.prevent_default();
                                        r();
                                    }
                                }
                            }
                            placeholder="输入消息，Enter 发送 / Shift+Enter 换行…"
                            rows="3"
                        ></textarea>
                        <div class="composer-bar-actions">
                            <button
                                type="button"
                                class="btn btn-muted btn-sm"
                                prop:disabled=move || !status_busy.get()
                                on:click={
                                    let t = Arc::clone(&trigger_stop);
                                    move |_| t()
                                }
                            >"停止"</button>
                            <button
                                type="button"
                                class="btn btn-primary btn-send-icon"
                                prop:disabled=move || status_busy.get() || !initialized.get()
                                on:click={
                                    let r = Arc::clone(&run_send_message);
                                    move |_| r()
                                }
                                title="发送"
                                aria-label="发送"
                            >
                                <svg
                                    class="btn-send-icon-svg"
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    xmlns="http://www.w3.org/2000/svg"
                                    aria-hidden="true"
                                >
                                    <path d="M22 2 11 13" />
                                    <path d="M22 2 15 22 11 13 2 9 22 2Z" />
                                </svg>
                            </button>
                        </div>
                        </div>
                    </div>
                </div>
    }
}
