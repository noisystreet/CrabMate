//! 「管理会话」模态框中的单行。

use leptos::prelude::*;

use crate::session_ops::{
    delete_session_after_confirm, export_session_json_for_id, export_session_markdown_for_id,
};
use crate::storage::ChatSession;

#[component]
pub fn SessionModalRow(
    id: String,
    title: String,
    message_count: usize,
    active: bool,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
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
                    move |_| {
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
                <span class="session-meta">{message_count}" 条"</span>
            </button>
            <div class="session-row-actions">
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    title="重命名"
                    on:click={
                        let sessions = sessions;
                        let id = id_rename.clone();
                        move |_| {
                            let default_title = sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.title.clone())
                                    .unwrap_or_default()
                            });
                            let Some(w) = web_sys::window() else {
                                return;
                            };
                            let raw = match w.prompt_with_message_and_default("会话标题", &default_title)
                            {
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
                    "重命名"
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    title="导出 JSON（ChatSessionFile v1）"
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
                    title="导出 Markdown"
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
                    title="删除此会话"
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
                            );
                        }
                    }
                >
                    "删除"
                </button>
            </div>
        </div>
    }
}
