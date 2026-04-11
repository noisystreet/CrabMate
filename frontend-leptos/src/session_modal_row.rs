//! 「管理会话」模态框中的单行。

use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::session_ops::{
    delete_session_after_confirm, export_session_json_for_id, export_session_markdown_for_id,
    flush_composer_draft_to_session, set_session_pinned, set_session_starred,
};
#[component]
pub fn SessionModalRow(
    id: String,
    title: String,
    message_count: usize,
    pinned: bool,
    starred: bool,
    active: bool,
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    draft: RwSignal<String>,
    /// 与主界面输入框共享；打开会话前把当前草稿写回上一活跃会话。
    composer_draft_buffer: Arc<Mutex<String>>,
    session_modal: RwSignal<bool>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let id_rename = id.clone();
    let id_star = id.clone();
    let id_pin = id.clone();
    let id_json = id.clone();
    let id_md = id.clone();
    let id_del = id.clone();
    let row_class = if active {
        "session-row active"
    } else {
        "session-row"
    };
    view! {
        <div class=row_class>
            <button
                type="button"
                class="session-open"
                on:click={
                    let id = id.clone();
                    let buf = Arc::clone(&composer_draft_buffer);
                    move |_| {
                        let prev = chat.active_id.get_untracked();
                        if !prev.is_empty() {
                            let t = buf.lock().unwrap().clone();
                            flush_composer_draft_to_session(chat.sessions, &prev, &t);
                        }
                        chat.active_id.set(id.clone());
                        draft.set(
                            chat.sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.draft.clone())
                                    .unwrap_or_default()
                            }),
                        );
                        chat.session_sync
                            .set(crate::session_sync::SessionSyncState::local_only());
                        session_modal.set(false);
                    }
                }
            >
                <span class="session-title">
                    {move || i18n::session_title_for_display(&title, locale.get())}
                </span>
                <span class="session-meta">{move || i18n::session_row_msg_count(locale.get(), message_count)}</span>
            </button>
            <div class="session-row-actions">
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    prop:title=move || {
                        if starred {
                            i18n::ctx_unstar_session(locale.get())
                        } else {
                            i18n::ctx_star_session(locale.get())
                        }
                    }
                    prop:aria-pressed=starred
                    on:click={
                        let sessions = chat.sessions;
                        let id = id_star.clone();
                        move |_| set_session_starred(sessions, &id, !starred)
                    }
                >
                    {if starred { "★" } else { "☆" }}
                </button>
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    prop:title=move || {
                        if pinned {
                            i18n::ctx_unpin_session(locale.get())
                        } else {
                            i18n::ctx_pin_session(locale.get())
                        }
                    }
                    prop:aria-pressed=pinned
                    on:click={
                        let sessions = chat.sessions;
                        let id = id_pin.clone();
                        move |_| set_session_pinned(sessions, &id, !pinned)
                    }
                >
                    "📌"
                </button>
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    prop:title=move || i18n::session_row_rename_title_attr(locale.get())
                    on:click={
                        let sessions = chat.sessions;
                        let id = id_rename.clone();
                        move |_| {
                            let loc = locale.get_untracked();
                            let default_title = sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.title.clone())
                                    .unwrap_or_default()
                            });
                            let Some(w) = web_sys::window() else {
                                return;
                            };
                            let raw = match w.prompt_with_message_and_default(
                                i18n::session_prompt_title_label(loc),
                                &default_title,
                            ) {
                                Ok(Some(s)) => s,
                                Ok(None) | Err(_) => return,
                            };
                            let t = raw.trim().to_string();
                            if t.is_empty() {
                                return;
                            }
                            sessions.update(|list| {
                                if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                                    s.title = t;
                                    s.updated_at = js_sys::Date::now() as i64;
                                }
                            });
                        }
                    }
                >
                    {move || i18n::session_row_rename_button(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:title=move || i18n::session_row_export_json_title(locale.get())
                    on:click={
                        let sessions = chat.sessions;
                        let id = id_json.clone();
                        move |_| {
                            export_session_json_for_id(
                                sessions,
                                &id,
                                locale.get_untracked(),
                                apply_assistant_display_filters.get_untracked(),
                            )
                        }
                    }
                >
                    "JSON"
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:title=move || i18n::session_row_export_md_title(locale.get())
                    on:click={
                        let sessions = chat.sessions;
                        let id = id_md.clone();
                        move |_| {
                            export_session_markdown_for_id(
                                sessions,
                                &id,
                                locale.get_untracked(),
                                apply_assistant_display_filters.get_untracked(),
                            )
                        }
                    }
                >
                    "MD"
                </button>
                <button
                    type="button"
                    class="btn btn-danger btn-sm"
                    prop:title=move || i18n::session_row_delete_title(locale.get())
                    on:click={
                        let sessions = chat.sessions;
                        let active_id = chat.active_id;
                        let draft = draft;
                        let session_sync = chat.session_sync;
                        let id = id_del.clone();
                        move |_| {
                            delete_session_after_confirm(
                                sessions,
                                active_id,
                                draft,
                                session_sync,
                                &id,
                                locale.get_untracked(),
                            );
                        }
                    }
                >
                    {move || i18n::session_row_delete_button(locale.get())}
                </button>
            </div>
        </div>
    }
}
