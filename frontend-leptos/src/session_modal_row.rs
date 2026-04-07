//! 「管理会话」模态框中的单行。

use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::session_ops::{
    delete_session_after_confirm, export_session_json_for_id, export_session_markdown_for_id,
    flush_composer_draft_to_session,
};
use crate::storage::ChatSession;

#[component]
pub fn SessionModalRow(
    id: String,
    title: String,
    message_count: usize,
    active: bool,
    locale: RwSignal<Locale>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    /// 与主界面输入框共享；打开会话前把当前草稿写回上一活跃会话。
    composer_draft_buffer: Arc<Mutex<String>>,
    conversation_id: RwSignal<Option<String>>,
    session_modal: RwSignal<bool>,
) -> impl IntoView {
    let id_rename = id.clone();
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
                        let prev = active_id.get_untracked();
                        if !prev.is_empty() {
                            let t = buf.lock().unwrap().clone();
                            flush_composer_draft_to_session(sessions, &prev, &t);
                        }
                        active_id.set(id.clone());
                        draft.set(
                            sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.draft.clone())
                                    .unwrap_or_default()
                            }),
                        );
                        conversation_id.set(None);
                        session_modal.set(false);
                    }
                }
            >
                <span class="session-title">{title}</span>
                <span class="session-meta">{move || i18n::session_row_msg_count(locale.get(), message_count)}</span>
            </button>
            <div class="session-row-actions">
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    prop:title=move || i18n::session_row_rename_title_attr(locale.get())
                    on:click={
                        let sessions = sessions;
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
                        let sessions = sessions;
                        let id = id_json.clone();
                        move |_| export_session_json_for_id(sessions, &id)
                    }
                >
                    "JSON"
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:title=move || i18n::session_row_export_md_title(locale.get())
                    on:click={
                        let sessions = sessions;
                        let id = id_md.clone();
                        move |_| export_session_markdown_for_id(sessions, &id)
                    }
                >
                    "MD"
                </button>
                <button
                    type="button"
                    class="btn btn-danger btn-sm"
                    prop:title=move || i18n::session_row_delete_title(locale.get())
                    on:click={
                        let sessions = sessions;
                        let active_id = active_id;
                        let draft = draft;
                        let conversation_id = conversation_id;
                        let id = id_del.clone();
                        move |_| {
                            delete_session_after_confirm(
                                sessions,
                                active_id,
                                draft,
                                conversation_id,
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
