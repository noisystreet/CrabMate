//! `GET /conversation/messages` 与本地 [`crate::storage::ChatSession`] 对齐（水合）。
//!
//! 从 `app/mod.rs` 抽出，避免根组件既管布局又实现同步语义。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_conversation_messages;
use crate::conversation_hydrate::stored_messages_from_conversation_api;
use crate::i18n::{self, Locale};
use crate::session_ops::title_from_user_prompt;
use crate::storage::StoredMessage;

use crate::chat_session_state::ChatSessionSignals;

fn count_user_role_bubbles(messages: &[StoredMessage]) -> usize {
    messages.iter().filter(|m| m.role == "user").count()
}

/// 订阅 `chat.session_hydrate_nonce`：流结束后由 composer 递增，拉取服务端快照并写回当前会话。
pub fn wire_session_hydration(
    initialized: RwSignal<bool>,
    web_ui_config_loaded: RwSignal<bool>,
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    selected_agent_role: RwSignal<Option<String>>,
) {
    Effect::new({
        let chat = chat;
        move |_| {
            if !initialized.get() || !web_ui_config_loaded.get() {
                return;
            }
            let aid = chat.active_id.get();
            if aid.is_empty() {
                return;
            }
            let nonce_at_start = chat.session_hydrate_nonce.get();
            let Some(cid) = chat.sessions.with_untracked(|list| {
                list.iter().find(|s| s.id == aid).and_then(|s| {
                    if s.messages
                        .iter()
                        .any(|m| m.state.as_deref() == Some("loading"))
                    {
                        return None;
                    }
                    let c = s
                        .server_conversation_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|x| !x.is_empty())?;
                    Some(c.to_string())
                })
            }) else {
                return;
            };
            let loc = locale.get_untracked();
            spawn_local(async move {
                let Ok(resp) = fetch_conversation_messages(&cid, loc).await else {
                    return;
                };
                if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
                    return;
                }
                let msgs = stored_messages_from_conversation_api(&resp.messages);
                if msgs.is_empty() && !resp.messages.is_empty() {
                    return;
                }
                let mut applied_hydration = false;
                chat.sessions.update(|list| {
                    if chat.active_id.get_untracked() != aid {
                        return;
                    }
                    let Some(s) = list.iter_mut().find(|x| x.id == aid) else {
                        return;
                    };
                    if s.messages
                        .iter()
                        .any(|m| m.state.as_deref() == Some("loading"))
                    {
                        return;
                    }
                    let still = s
                        .server_conversation_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|x| !x.is_empty());
                    if still != Some(cid.as_str()) {
                        return;
                    }
                    if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
                        return;
                    }
                    let local_users = count_user_role_bubbles(&s.messages);
                    let hydrated_users = count_user_role_bubbles(&msgs);
                    if !s.messages.is_empty() && msgs.is_empty() {
                        return;
                    }
                    if local_users > 0 && hydrated_users < local_users {
                        return;
                    }
                    // 保留本地的工具消息（is_tool=true 的消息，如分层执行通过 SSE 添加的工具卡片）
                    let server_msg_ids: std::collections::HashSet<_> =
                        msgs.iter().map(|m| m.id.as_str()).collect();
                    let local_tool_messages: Vec<StoredMessage> = s
                        .messages
                        .iter()
                        .filter(|m| m.is_tool && !server_msg_ids.contains(m.id.as_str()))
                        .cloned()
                        .collect();
                    let mut new_messages = msgs;
                    new_messages.extend(local_tool_messages);
                    s.messages = new_messages;
                    s.server_revision = Some(resp.revision);
                    applied_hydration = true;
                    if let Some(role) = resp
                        .active_agent_role
                        .as_deref()
                        .map(str::trim)
                        .filter(|r| !r.is_empty())
                    {
                        selected_agent_role.set(Some(role.to_string()));
                    }
                    let user_count = s.messages.iter().filter(|m| m.role == "user").count();
                    if user_count == 1 && i18n::is_default_session_title(&s.title) {
                        if let Some(u) = s.messages.iter().find(|m| m.role == "user") {
                            s.title = title_from_user_prompt(&u.text);
                        }
                    }
                });
                if !applied_hydration {
                    return;
                }
                if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
                    return;
                }
                chat.session_sync.update(|st| {
                    if st.conversation_id.as_deref().map(str::trim) == Some(cid.as_str()) {
                        st.apply_saved_revision(resp.revision);
                    }
                });
            });
        }
    });
}
