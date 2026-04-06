//! 中部聊天列：消息列表、输入框、查找/多选入口。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use super::chat_message_render::{
    ChatChunk, chat_message_row, chunk_messages, tool_run_group_view,
};
use super::scroll_guard::MessagesScrollFromEffectGuard;
use crate::app_prefs::AUTO_SCROLL_RESUME_GAP_PX;
use crate::session_ops::{clamp_session_ctx_menu_pos, selected_text_in_messages_for_context_copy};
use crate::storage::ChatSession;

#[allow(clippy::too_many_arguments)]
pub fn chat_column_view(
    messages_scroller: NodeRef<leptos::html::Div>,
    auto_scroll_chat: RwSignal<bool>,
    messages_scroll_from_effect: RwSignal<bool>,
    last_messages_scroll_top: RwSignal<i32>,
    session_context_menu: RwSignal<Option<crate::session_ops::SessionContextAnchor>>,
    chat_export_ctx_menu: RwSignal<Option<(f64, f64, Option<String>)>>,
    chat_find_panel_open: RwSignal<bool>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    expanded_tool_run_heads: RwSignal<HashSet<String>>,
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
                            let selection = ev
                                .current_target()
                                .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
                                .and_then(|el| selected_text_in_messages_for_context_copy(&el));
                            chat_export_ctx_menu.set(Some((x, y, selection)));
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
                                        chunk_messages(&msgs)
                                            .into_iter()
                                            .map(|chunk| match chunk {
                                                ChatChunk::Single { idx, msg } => chat_message_row(
                                                    idx,
                                                    msg,
                                                    sessions,
                                                    active_id,
                                                    expanded_long_assistant_ids,
                                                    chat_find_query,
                                                    chat_find_match_ids,
                                                    chat_find_cursor,
                                                    bubble_md_select_mode,
                                                    bubble_md_selected_ids,
                                                    status_busy,
                                                    conversation_id,
                                                    conversation_revision,
                                                    regen_stream_after_truncate,
                                                    retry_assistant_target,
                                                    status_err,
                                                )
                                                .into_any(),
                                                ChatChunk::ToolGroup { head_id, items } => {
                                                    tool_run_group_view(
                                                        head_id,
                                                        items,
                                                        expanded_tool_run_heads,
                                                        chat_find_query,
                                                        chat_find_match_ids,
                                                        sessions,
                                                        active_id,
                                                        expanded_long_assistant_ids,
                                                        bubble_md_select_mode,
                                                        bubble_md_selected_ids,
                                                        chat_find_cursor,
                                                        status_busy,
                                                        conversation_id,
                                                        conversation_revision,
                                                        regen_stream_after_truncate,
                                                        retry_assistant_target,
                                                        status_err,
                                                    )
                                                    .into_any()
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
