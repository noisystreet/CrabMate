//! 管理会话模态框。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::i18n;
use crate::session_modal_row::{SessionModalRow, SessionModalRowBundle};
use crate::session_sort::sorted_sessions_clone;

use super::app_shell_ctx::SessionListModalSignals;
use super::shell_runtime_context::expect_chat_shell_ctx;

#[component]
fn SessionListModalPanel(session_modal: RwSignal<bool>) -> impl IntoView {
    let shell = expect_chat_shell_ctx();
    let chat = shell.chat;
    let draft = shell.composer.draft;
    let locale = shell.locale;
    let apply_assistant_display_filters = shell.apply_assistant_display_filters;
    let dialog_ref = NodeRef::<Div>::new();

    Effect::new({
        let dialog_ref = dialog_ref.clone();
        move |_| {
            if !session_modal.get() {
                return;
            }
            let r = dialog_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = r.get() {
                    focus_first_in_modal_container(el.as_ref());
                }
            });
        }
    });

    view! {
        <div
            class="modal"
            node_ref=dialog_ref
            role="dialog"
            aria-modal="true"
            aria-labelledby="session-list-modal-title"
            data-testid="session-list-modal"
            tabindex="-1"
            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Tab" {
                    if let Some(el) = dialog_ref.get() {
                        trap_tab_in_container(&ev, el.as_ref());
                    }
                }
            }
        >
            <div class="modal-head">
                <h2 class="modal-title" id="session-list-modal-title">{move || i18n::session_modal_title(locale.get())}</h2>
                <span class="modal-badge">{move || i18n::session_modal_badge(locale.get())}</span>
                <span class="modal-head-spacer"></span>
                <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| session_modal.set(false)>
                    {move || i18n::settings_close(locale.get())}
                </button>
            </div>
            <div class="modal-body">
                <p class="modal-hint">
                    {move || i18n::session_modal_hint(locale.get())}
                </p>
                {move || {
                    sorted_sessions_clone(&chat.sessions.get())
                        .into_iter()
                        .map(|s| {
                            let id = s.id.clone();
                            let active = chat.active_id.get() == id;
                            let row_title = s.title.clone();
                            let pinned = s.pinned;
                            let starred = s.starred;
                            view! {
                                <SessionModalRow row=SessionModalRowBundle {
                                    id: id.clone(),
                                    title: row_title,
                                    message_count: s.messages.len(),
                                    pinned,
                                    starred,
                                    active,
                                    locale,
                                    chat,
                                    draft,
                                    session_modal,
                                    apply_assistant_display_filters,
                                } />
                            }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

/// 避免 `view!` 中 backdrop 子树闭包捕获移动导致 `FnOnce`。
#[component]
fn SessionListModalBackdrop(session_modal: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="modal-backdrop" on:click=move |_| session_modal.set(false)>
            <SessionListModalPanel session_modal=session_modal />
        </div>
    }
}

pub fn session_list_modal_view(signals: SessionListModalSignals) -> impl IntoView {
    let SessionListModalSignals { session_modal } = signals;
    view! {
        <Show when=move || session_modal.get()>
            <SessionListModalBackdrop session_modal=session_modal />
        </Show>
    }
}
