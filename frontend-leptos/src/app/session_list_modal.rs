//! 管理会话模态框。

use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::session_modal_row::SessionModalRow;
use crate::storage::ChatSession;

#[component]
fn SessionListModalPanel(
    session_modal: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    composer_draft_buffer: Arc<Mutex<String>>,
) -> impl IntoView {
    view! {
        <div class="modal" on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()>
            <div class="modal-head">
                <h2 class="modal-title">"会话"</h2>
                <span class="modal-badge">"本地"</span>
                <span class="modal-head-spacer"></span>
                <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| session_modal.set(false)>
                    "关闭"
                </button>
            </div>
            <div class="modal-body">
                <p class="modal-hint">
                    "本地保存在浏览器；可导出为与 CLI save-session 同形的 JSON / Markdown 下载。"
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
                sessions=sessions
                active_id=active_id
                draft=draft
                conversation_id=conversation_id
                composer_draft_buffer=composer_draft_buffer.clone()
            />
        </Show>
    }
}
