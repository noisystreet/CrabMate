//! 中部聊天列：消息列表、输入框、查找/多选入口。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Textarea;
use leptos::prelude::{StoredValue, *};
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use super::chat_message_render::{
    ChatChunk, chat_message_row, chunk_messages, staged_timeline_group_view, tool_run_group_view,
};
use super::scroll_guard::MessagesScrollFromEffectGuard;
use super::timeline_panel::timeline_panel_view;
use crate::api::upload_files_multipart;
use crate::app_prefs::AUTO_SCROLL_RESUME_GAP_PX;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::{self, Locale};
use crate::session_ops::{clamp_session_ctx_menu_pos, selected_text_in_messages_for_context_copy};
use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

#[allow(clippy::too_many_arguments)]
pub fn chat_column_view(
    locale: RwSignal<Locale>,
    messages_scroller: NodeRef<leptos::html::Div>,
    auto_scroll_chat: RwSignal<bool>,
    messages_scroll_from_effect: RwSignal<bool>,
    last_messages_scroll_top: RwSignal<i32>,
    session_context_menu: RwSignal<Option<crate::session_ops::SessionContextAnchor>>,
    chat_export_ctx_menu: RwSignal<Option<(f64, f64, Option<String>)>>,
    chat_find_panel_open: RwSignal<bool>,
    timeline_panel_expanded: RwSignal<bool>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    expanded_tool_run_heads: RwSignal<HashSet<String>>,
    expanded_staged_timeline_heads: RwSignal<HashSet<String>>,
    chat_find_query: RwSignal<String>,
    chat_find_match_ids: RwSignal<Vec<String>>,
    chat_find_cursor: RwSignal<usize>,
    composer_input_ref: NodeRef<Textarea>,
    composer_buf_ta: Arc<Mutex<String>>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    run_send_message: Arc<dyn Fn() + Send + Sync>,
    trigger_stop: Arc<dyn Fn() + Send + Sync>,
    status_busy: RwSignal<bool>,
    initialized: RwSignal<bool>,
    session_sync: RwSignal<SessionSyncState>,
    regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    retry_assistant_target: RwSignal<Option<String>>,
    status_err: RwSignal<Option<String>>,
    markdown_render: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let run_send_clarify_sv = StoredValue::new(run_send_message.clone());
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
                            prop:title=move || i18n::bubble_md_toggle_title(locale.get())
                            prop:aria-label=move || i18n::bubble_md_toggle_aria(locale.get())
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
                            prop:title=move || i18n::chat_find_toggle_title(locale.get())
                            prop:aria-label=move || i18n::chat_find_toggle_aria(locale.get())
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
                        {timeline_panel_view(
                            locale,
                            sessions,
                            active_id,
                            timeline_panel_expanded,
                            auto_scroll_chat,
                        )}
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
                                                    <p class="messages-empty-title">
                                                        {move || i18n::chat_empty_title(locale.get())}
                                                    </p>
                                                    <p class="messages-empty-lead">
                                                        {move || i18n::chat_empty_lead(locale.get())}
                                                    </p>
                                                    <ul class="messages-empty-tips">
                                                        <li>{move || i18n::chat_empty_tip1(locale.get())}</li>
                                                        <li>{move || i18n::chat_empty_tip2(locale.get())}</li>
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
                                                        session_sync,
                                                        regen_stream_after_truncate,
                                                        retry_assistant_target,
                                                        status_err,
                                                        auto_scroll_chat,
                                                        locale,
                                                        markdown_render,
                                                        apply_assistant_display_filters,
                                                    )
                                                    .into_any()
                                                }
                                                ChatChunk::StagedTimelineGroup { head_id, items } => {
                                                    staged_timeline_group_view(
                                                        head_id,
                                                        items,
                                                        expanded_staged_timeline_heads,
                                                        chat_find_query,
                                                        chat_find_match_ids,
                                                        sessions,
                                                        active_id,
                                                        expanded_long_assistant_ids,
                                                        bubble_md_select_mode,
                                                        bubble_md_selected_ids,
                                                        chat_find_cursor,
                                                        status_busy,
                                                        session_sync,
                                                        regen_stream_after_truncate,
                                                        retry_assistant_target,
                                                        status_err,
                                                        auto_scroll_chat,
                                                        locale,
                                                        markdown_render,
                                                        apply_assistant_display_filters,
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
                        <input
                            type="file"
                            class="composer-file-input-hidden"
                            id="composer-image-input"
                            accept="image/png,image/jpeg,image/jpg,image/webp,image/gif"
                            multiple
                            on:change=move |ev: web_sys::Event| {
                                let Some(t) = ev.target() else {
                                    return;
                                };
                                let Ok(input) = t.dyn_into::<web_sys::HtmlInputElement>() else {
                                    return;
                                };
                                let files = input.files();
                                let Some(list) = files else {
                                    return;
                                };
                                let n = list.length();
                                if n == 0 {
                                    return;
                                }
                                let form = web_sys::FormData::new().expect("FormData");
                                    for i in 0..n {
                                    if let Some(f) = list.item(i) {
                                        let name = f.name();
                                        let _ = form.append_with_blob_and_filename("file", &f, &name);
                                    }
                                }
                                spawn_local(async move {
                                    match upload_files_multipart(&form).await {
                                        Ok(urls) => {
                                            pending_images.update(|v| {
                                                for u in urls {
                                                    if v.len() >= 6 {
                                                        break;
                                                    }
                                                    if !v.contains(&u) {
                                                        v.push(u);
                                                    }
                                                }
                                            });
                                        }
                                        Err(e) => {
                                            status_err.set(Some(e));
                                        }
                                    }
                                });
                                input.set_value("");
                            }
                        />
                        <div class="composer-pending-images" data-testid="composer-pending-images">
                            {move || {
                                let imgs = pending_images.get();
                                if imgs.is_empty() {
                                    return view! { <span></span> }.into_any();
                                }
                                imgs.iter()
                                    .map(|url| {
                                        let u = url.clone();
                                        let u_rm = url.clone();
                                        view! {
                                            <div class="composer-pending-img-wrap">
                                                <img class="composer-pending-img" src=u alt="" />
                                                <button
                                                    type="button"
                                                    class="composer-pending-img-remove"
                                                    prop:aria-label=move || i18n::composer_remove_image_aria(locale.get())
                                                    on:click=move |_| {
                                                        pending_images.update(|v| v.retain(|x| x != &u_rm));
                                                    }
                                                >"×"</button>
                                            </div>
                                        }
                                        .into_any()
                                    })
                                    .collect_view()
                                    .into_any()
                            }}
                        </div>
                        <Show when=move || pending_clarification.get().is_some()>
                            <div class="composer-clarification-panel" data-testid="composer-clarification-panel">
                                {move || {
                                    let Some(form) = pending_clarification.get() else {
                                        return view! { <span></span> }.into_any();
                                    };
                                    let intro = form.intro.clone();
                                    let loc = locale.get();
                                    let n = form.fields.len();
                                    let pc = pending_clarification;
                                    if form.values.len() != n {
                                        pc.update(|opt| {
                                            if let Some(fm) = opt.as_mut() {
                                                fm.values.resize(n, String::new());
                                            }
                                        });
                                    }
                                    view! {
                                        <div class="composer-clarification-title">
                                            {i18n::clarification_panel_title(loc)}
                                        </div>
                                        <p class="composer-clarification-intro">{intro}</p>
                                        <div class="composer-clarification-fields">
                                            {form
                                                .fields
                                                .iter()
                                                .enumerate()
                                                .map(|(i, f)| {
                                                    let label = f.label.clone();
                                                    let hint = f.hint.clone();
                                                    let req = f.required;
                                                    let idx = i;
                                                    let pc2 = pc;
                                                    view! {
                                                        <label class="composer-clarification-field">
                                                            <span class="composer-clarification-label">
                                                                {label.clone()}
                                                                {if req {
                                                                    i18n::clarification_required_suffix(loc).to_string()
                                                                } else {
                                                                    String::new()
                                                                }}
                                                            </span>
                                                            {match &hint {
                                                                Some(h) => view! {
                                                                    <span class="composer-clarification-hint">{h.clone()}</span>
                                                                }
                                                                .into_any(),
                                                                None => view! { <span></span> }.into_any(),
                                                            }}
                                                            <input
                                                                type="text"
                                                                class="composer-clarification-input"
                                                                prop:value=move || {
                                                                    pc2.with(|opt| {
                                                                        opt.as_ref()
                                                                            .and_then(|fm| fm.values.get(idx))
                                                                            .cloned()
                                                                            .unwrap_or_default()
                                                                    })
                                                                }
                                                                on:input=move |ev| {
                                                                    let t = event_target_value(&ev);
                                                                    pc2.update(|opt| {
                                                                        if let Some(fm) = opt.as_mut()
                                                                            && fm.values.len() > idx
                                                                        {
                                                                            fm.values[idx] = t;
                                                                        }
                                                                    });
                                                                }
                                                            />
                                                        </label>
                                                    }
                                                    .into_any()
                                                })
                                                .collect_view()}
                                        </div>
                                        <div class="composer-clarification-actions">
                                            <button
                                                type="button"
                                                class="btn btn-muted btn-sm"
                                                prop:disabled=move || status_busy.get()
                                                on:click=move |_| pending_clarification.set(None)
                                            >
                                                {move || i18n::clarification_dismiss(locale.get())}
                                            </button>
                                            <button
                                                type="button"
                                                class="btn btn-primary btn-sm"
                                                prop:disabled=move || status_busy.get()
                                                on:click=move |_| {
                                                    run_send_clarify_sv.get_value()();
                                                }
                                            >
                                                {move || i18n::clarification_submit(locale.get())}
                                            </button>
                                        </div>
                                    }
                                    .into_any()
                                }}
                            </div>
                        </Show>
                        <div class="composer-input-row">
                        <textarea
                            class="composer-input"
                            data-testid="chat-composer-input"
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
                            prop:placeholder=move || i18n::composer_ph(locale.get())
                            rows="3"
                        ></textarea>
                        <div class="composer-bar-actions">
                            <label
                                class="btn btn-muted btn-sm composer-attach-label"
                                for="composer-image-input"
                                prop:title=move || i18n::composer_attach_image_aria(locale.get())
                                prop:aria-label=move || i18n::composer_attach_image_aria(locale.get())
                            >
                                <svg
                                    viewBox="0 0 24 24"
                                    fill="none"
                                    stroke="currentColor"
                                    stroke-width="2"
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    class="composer-attach-icon"
                                    aria-hidden="true"
                                >
                                    <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                                    <circle cx="8.5" cy="8.5" r="1.5" />
                                    <path d="m21 15-3.5-3.5a2 2 0 0 0-2.83 0L6 21" />
                                </svg>
                            </label>
                            <button
                                type="button"
                                class="btn btn-muted btn-sm"
                                prop:disabled=move || !status_busy.get()
                                on:click={
                                    let t = Arc::clone(&trigger_stop);
                                    move |_| t()
                                }
                            >{move || i18n::composer_stop(locale.get())}</button>
                            <button
                                type="button"
                                class="btn btn-primary btn-send-icon"
                                data-testid="chat-send-button"
                                prop:disabled=move || status_busy.get() || !initialized.get()
                                on:click={
                                    let r = Arc::clone(&run_send_message);
                                    move |_| r()
                                }
                                prop:title=move || i18n::composer_send_aria(locale.get())
                                prop:aria-label=move || i18n::composer_send_aria(locale.get())
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
                </div>
    }
}
