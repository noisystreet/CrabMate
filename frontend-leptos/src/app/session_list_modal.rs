//! 管理会话模态框。

use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::i18n::{self, Locale};
use crate::session_modal_row::SessionModalRow;
use crate::storage::ChatSession;

#[component]
fn SessionListModalPanel(
    session_modal: RwSignal<bool>,
    locale: RwSignal<Locale>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    composer_draft_buffer: Arc<Mutex<String>>,
) -> impl IntoView {
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
                    let buf = composer_draft_buffer.clone();
                    sessions
                        .get()
                        .into_iter()
                        .map(|s| {
                            let id = s.id.clone();
                            let active = active_id.get() == id;
                            let row_buf = Arc::clone(&buf);
                            view! {
                                <SessionModalRow
                                    id=id.clone()
                                    title=s.title.clone()
                                    message_count=s.messages.len()
                                    active=active
                                    locale=locale
                                    sessions=sessions
                                    active_id=active_id
                                    draft=draft
                                    composer_draft_buffer=row_buf
                                    conversation_id=conversation_id
                                    session_modal=session_modal
                                />
                            }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

/// 避免 `view!` 中 backdrop 子树与 `composer_draft_buffer` 移动导致 `FnOnce`。
#[component]
fn SessionListModalBackdrop(
    session_modal: RwSignal<bool>,
    locale: RwSignal<Locale>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    composer_draft_buffer: Arc<Mutex<String>>,
) -> impl IntoView {
    view! {
        <div class="modal-backdrop" on:click=move |_| session_modal.set(false)>
            <SessionListModalPanel
                session_modal=session_modal
                locale=locale
                sessions=sessions
                active_id=active_id
                draft=draft
                conversation_id=conversation_id
                composer_draft_buffer=composer_draft_buffer
            />
        </div>
    }
}

pub fn session_list_modal_view(
    session_modal: RwSignal<bool>,
    locale: RwSignal<Locale>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    composer_draft_buffer: Arc<Mutex<String>>,
) -> impl IntoView {
    view! {
        <Show when=move || session_modal.get()>
            <SessionListModalBackdrop
                session_modal=session_modal
                locale=locale
                sessions=sessions
                active_id=active_id
                draft=draft
                conversation_id=conversation_id
                composer_draft_buffer=composer_draft_buffer.clone()
            />
        </Show>
    }
}
