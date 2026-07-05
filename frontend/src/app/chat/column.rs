//! 中部聊天列：消息列表、输入框、查找入口。

use std::collections::HashMap;
use std::sync::Arc;

use leptos::prelude::{StoredValue, *};
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use super::column_keyboard::ChatColumnHomeEndNav;
use super::composer_input_stack::ComposerInputStack;
use super::handles::{ChatColumnShell, ChatComposerPaneSignals, ChatMessagesPaneSignals};
use super::message_group_views::ToolRunGroupSignals;
use super::messages_list::{ChatMessagesList, ChatMessagesListSignals};
use super::scroll_shell::{
    ChatScrollShellSignals, on_messages_scroll_event, on_messages_wheel_follow_intent,
};
use super::tail_loading_memo::tail_loading_assistant_mid_memo;
use super::timeline::timeline_panel_view;
use crate::api::upload_files_multipart;
use crate::i18n;
#[component]
fn ChatMessagesScrollShell(
    scroll_shell: ChatScrollShellSignals,
    children: Children,
) -> impl IntoView {
    view! {
        <div
            class="messages"
            data-testid="chat-messages-scroller"
            node_ref=scroll_shell.messages_scroller
            on:wheel=move |ev: web_sys::WheelEvent| {
                on_messages_wheel_follow_intent(scroll_shell.auto_scroll_chat, ev);
            }
            on:scroll=move |ev: web_sys::Event| {
                on_messages_scroll_event(scroll_shell, ev);
            }
        >
            {children()}
        </div>
    }
}

#[component]
fn ChatMessagesThreadBody(
    pane: ChatMessagesPaneSignals,
    tool_run_group_signals: ToolRunGroupSignals,
) -> impl IntoView {
    let ChatMessagesPaneSignals {
        locale,
        timeline_panel_expanded,
        chat,
        scroll_shell,
        ..
    } = pane;

    let sessions = chat.sessions;
    let active_id = chat.active_id;
    let auto_scroll_chat = tool_run_group_signals.auto_scroll_chat;

    view! {
        <div class="chat-thread">
            {timeline_panel_view(
                locale,
                sessions,
                active_id,
                timeline_panel_expanded,
                auto_scroll_chat,
            )}
            <div class="messages-inner">
                <Show
                    when=move || {
                        let id = active_id.get();
                        sessions.with(|list| {
                            list.iter()
                                .find(|s| s.id == id)
                                .map(|s| !s.messages.is_empty())
                                .unwrap_or(false)
                        })
                    }
                    fallback=move || {
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
                    }
                >
                    <ChatMessagesList signals=ChatMessagesListSignals {
                        chat,
                        sessions,
                        active_id,
                        locale,
                        scroll_shell,
                        tool_run_group_signals,
                    } />
                </Show>
            </div>
        </div>
    }
}

#[component]
fn ChatMessagesPane(signals: ChatMessagesPaneSignals) -> impl IntoView {
    let ChatMessagesPaneSignals {
        scroll_shell,
        timeline_panel_expanded: _,
        chat,
        collapsed_long_assistant_ids,
        collapsed_tool_run_heads,
        tool_detail_expanded_ids,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        stream_turn_busy_ui,
        stream_follow_up,
        status_err,
        locale,
        markdown_render,
        apply_assistant_display_filters,
    } = signals;

    let auto_scroll_chat = scroll_shell.auto_scroll_chat;

    let tail_loading_assistant_mid = tail_loading_assistant_mid_memo(chat);

    let tool_run_group_signals = ToolRunGroupSignals {
        collapsed_tool_run_heads,
        tool_detail_expanded_ids,
        chat_find_query,
        chat_find_match_ids,
        chat,
        collapsed_long_assistant_ids,
        chat_find_cursor,
        stream_turn_busy_ui,
        tail_loading_assistant_mid,
        stream_follow_up,
        status_err,
        auto_scroll_chat,
        locale,
        markdown_render,
        apply_assistant_display_filters,
        row_state_map: Memo::new(move |_| HashMap::new()),
    };

    view! {
        <ChatMessagesScrollShell scroll_shell>
            <ChatMessagesThreadBody pane=signals tool_run_group_signals />
        </ChatMessagesScrollShell>
    }
}

fn handle_composer_image_input_change(
    ev: web_sys::Event,
    locale: RwSignal<crate::i18n::Locale>,
    pending_images: RwSignal<Vec<String>>,
    status_err: RwSignal<Option<String>>,
) {
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
        match upload_files_multipart(&form, locale.get_untracked()).await {
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

#[component]
fn ComposerImageInput(
    locale: RwSignal<crate::i18n::Locale>,
    pending_images: RwSignal<Vec<String>>,
    status_err: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <input
            type="file"
            class="composer-file-input-hidden"
            id="composer-image-input"
            accept="image/png,image/jpeg,image/jpg,image/webp,image/gif"
            multiple
            on:change=move |ev: web_sys::Event| {
                handle_composer_image_input_change(ev, locale, pending_images, status_err);
            }
        />
    }
}

#[component]
fn ComposerPendingImagesRow(
    locale: RwSignal<crate::i18n::Locale>,
    pending_images: RwSignal<Vec<String>>,
) -> impl IntoView {
    view! {
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
                                    on:click=move |_| pending_images.update(|v| v.retain(|x| x != &u_rm))
                                >"×"</button>
                            </div>
                        }
                        .into_any()
                    })
                    .collect_view()
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn ComposerClarificationPanel(
    locale: RwSignal<crate::i18n::Locale>,
    pending_clarification: RwSignal<Option<crate::clarification_form::PendingClarificationForm>>,
    stream_turn_busy_ui: Memo<bool>,
    run_send_clarify_sv: StoredValue<Arc<dyn Fn() + Send + Sync>>,
) -> impl IntoView {
    view! {
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
                                                data-testid="composer-clarification-input"
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
                                prop:disabled=move || stream_turn_busy_ui.get()
                                on:click=move |_| pending_clarification.set(None)
                            >
                                {move || i18n::clarification_dismiss(locale.get())}
                            </button>
                            <button
                                type="button"
                                class="btn btn-primary btn-sm"
                                data-testid="composer-clarification-submit"
                                prop:disabled=move || stream_turn_busy_ui.get()
                                on:click=move |_| run_send_clarify_sv.get_value()()
                            >
                                {move || i18n::clarification_submit(locale.get())}
                            </button>
                        </div>
                    }
                    .into_any()
                }}
            </div>
        </Show>
    }
}

#[component]
fn ChatComposerPane(signals: ChatComposerPaneSignals) -> impl IntoView {
    let ChatComposerPaneSignals {
        locale,
        pending_images,
        pending_clarification,
        stream_turn_busy_ui,
        status_err,
        run_send_message,
        run_send_clarify_sv,
        trigger_stop,
        initialized,
        composer_input_ref,
        draft,
        composer_mirror_html,
        composer_mirror_scroll_top,
    } = signals;

    view! {
        <div class="composer composer-ds">
            <div class="composer-inner-ds">
                <ComposerImageInput
                    locale=locale
                    pending_images=pending_images
                    status_err=status_err
                />
                <ComposerPendingImagesRow locale=locale pending_images=pending_images />
                <ComposerClarificationPanel
                    locale=locale
                    pending_clarification=pending_clarification
                    stream_turn_busy_ui=stream_turn_busy_ui
                    run_send_clarify_sv=run_send_clarify_sv
                />
                <div class="composer-input-row">
                    <ComposerInputStack
                        composer_input_ref=composer_input_ref
                        draft=draft
                        composer_mirror_html=composer_mirror_html
                        composer_mirror_scroll_top=composer_mirror_scroll_top
                        run_send_message=run_send_message.clone()
                        locale=locale
                    />
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
                            prop:disabled=move || !stream_turn_busy_ui.get()
                            on:click={
                                let t = Arc::clone(&trigger_stop);
                                move |_| t()
                            }
                        >{move || i18n::composer_stop(locale.get())}</button>
                        <button
                            type="button"
                            class="btn btn-primary btn-send-icon"
                            data-testid="chat-send-button"
                            prop:disabled=move || stream_turn_busy_ui.get() || !initialized.get()
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
    }
}

pub fn chat_column_view(shell: ChatColumnShell) -> impl IntoView {
    let home_end_nav = ChatColumnHomeEndNav::from_composer(&shell.app.chat_composer);
    let run_send_clarify_sv = StoredValue::new(shell.run_send_message.clone());
    view! {
                <div
                    class="chat-column"
                    on:keydown:capture=home_end_nav.keydown_handler()
                >
                    <ChatMessagesPane signals=shell.messages_pane_signals() />
                    <ChatComposerPane signals=shell.composer_pane_signals(run_send_clarify_sv) />
                </div>
    }
}
