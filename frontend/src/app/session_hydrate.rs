//! `GET /conversation/messages` 与本地 [`crate::storage::ChatSession`] 对齐（水合）。
//!
//! 从 `app/mod.rs` 抽出，避免根组件既管布局又实现同步语义。

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_conversation_messages;
use crate::conversation_hydrate::{
    ConversationMessagesResponse, stored_messages_from_conversation_api,
};
use crate::i18n::{self, Locale};
use crate::session_ops::title_from_user_prompt;
use crate::storage::{ChatSession, StoredMessage};

use crate::chat_session_state::ChatSessionSignals;

use super::app_bootstrap_phase::AppBootstrapPhase;

fn count_user_role_bubbles(messages: &[StoredMessage]) -> usize {
    messages.iter().filter(|m| m.role == "user").count()
}

fn messages_contain_loading(messages: &[StoredMessage]) -> bool {
    messages
        .iter()
        .any(|m| m.state.as_ref().is_some_and(|s| s.is_loading()))
}

fn trimmed_server_conversation_id(session: &ChatSession) -> Option<&str> {
    session
        .server_conversation_id
        .as_deref()
        .map(str::trim)
        .filter(|x| !x.is_empty())
}

/// 服务端快照中不存在的本地消息：工具卡与 TimelineLog，须保留并与快照合并。
fn local_messages_preserved_after_hydrate(
    server_msgs: &[StoredMessage],
    local_msgs: &[StoredMessage],
) -> Vec<StoredMessage> {
    let server_msg_ids: HashSet<_> = server_msgs.iter().map(|m| m.id.as_str()).collect();
    local_msgs
        .iter()
        .filter(|m| {
            if m.is_tool && !server_msg_ids.contains(m.id.as_str()) {
                return true;
            }
            if let Some(ref state) = m.state {
                if state.is_local_timeline_snapshot_row() && !server_msg_ids.contains(m.id.as_str())
                {
                    return true;
                }
            }
            false
        })
        .cloned()
        .collect()
}

/// 将服务端快照合并进当前会话时的守卫结果（原 `merge_*` 各 `return false` 路径的显式命名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionHydrationMergeOutcome {
    Applied,
    SkippedActiveSessionMismatch,
    SkippedLoadingPlaceholders,
    SkippedConversationIdMismatch,
    SkippedHydrateNonceMismatch,
    SkippedEmptyHydrateAgainstLocalMessages,
    SkippedHydratedUserRegression,
}

impl SessionHydrationMergeOutcome {
    #[must_use]
    pub(crate) const fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// 合并水合快照时的标识与会话状态（避免 `merge_*` 长参数列表）。
struct MergeHydrationIntoActiveSessionArgs<'a> {
    session: &'a mut ChatSession,
    aid: &'a str,
    cid: &'a str,
    hydrated: Vec<StoredMessage>,
    resp: &'a ConversationMessagesResponse,
    nonce_at_start: u64,
    current_nonce: u64,
    active_id: &'a str,
    selected_agent_role: RwSignal<Option<String>>,
}

/// 水合合并前的**有序守卫**：返回 `Err(skip)` 时调用方应直接返回对应 [`SessionHydrationMergeOutcome`]。
fn try_hydration_merge_precheck(
    session: &ChatSession,
    aid: &str,
    cid: &str,
    hydrated: &[StoredMessage],
    nonce_at_start: u64,
    current_nonce: u64,
    active_id: &str,
) -> Result<(), SessionHydrationMergeOutcome> {
    if active_id != aid {
        return Err(SessionHydrationMergeOutcome::SkippedActiveSessionMismatch);
    }
    if messages_contain_loading(&session.messages) {
        return Err(SessionHydrationMergeOutcome::SkippedLoadingPlaceholders);
    }
    let still = trimmed_server_conversation_id(session);
    if still != Some(cid) {
        return Err(SessionHydrationMergeOutcome::SkippedConversationIdMismatch);
    }
    if current_nonce != nonce_at_start {
        return Err(SessionHydrationMergeOutcome::SkippedHydrateNonceMismatch);
    }
    let local_users = count_user_role_bubbles(&session.messages);
    let hydrated_users = count_user_role_bubbles(hydrated);
    if !session.messages.is_empty() && hydrated.is_empty() {
        return Err(SessionHydrationMergeOutcome::SkippedEmptyHydrateAgainstLocalMessages);
    }
    if local_users > 0 && hydrated_users < local_users {
        return Err(SessionHydrationMergeOutcome::SkippedHydratedUserRegression);
    }
    Ok(())
}

/// 将 `GET /conversation/messages` 结果合并进当前会话；[`SessionHydrationMergeOutcome::Applied`] 表示已写 `messages` / `server_revision` 等。
fn merge_hydration_into_active_session(
    args: MergeHydrationIntoActiveSessionArgs<'_>,
) -> SessionHydrationMergeOutcome {
    let MergeHydrationIntoActiveSessionArgs {
        session,
        aid,
        cid,
        hydrated,
        resp,
        nonce_at_start,
        current_nonce,
        active_id,
        selected_agent_role,
    } = args;
    if let Err(out) = try_hydration_merge_precheck(
        session,
        aid,
        cid,
        &hydrated,
        nonce_at_start,
        current_nonce,
        active_id,
    ) {
        return out;
    }
    let mut new_messages = hydrated;
    let preserved = local_messages_preserved_after_hydrate(&new_messages, &session.messages);
    new_messages.extend(preserved);
    session.messages = new_messages;
    session.server_revision = Some(resp.revision);
    if let Some(role) = resp
        .active_agent_role
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        selected_agent_role.set(Some(role.to_string()));
    }
    let user_count = session.messages.iter().filter(|m| m.role == "user").count();
    if user_count == 1 && i18n::is_default_session_title(&session.title) {
        if let Some(u) = session.messages.iter().find(|m| m.role == "user") {
            session.title = title_from_user_prompt(&u.text);
        }
    }
    SessionHydrationMergeOutcome::Applied
}

fn restore_reasoning_after_hydration(chat: &ChatSessionSignals, aid: &str, nonce_at_start: u64) {
    if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
        return;
    }
    let preserved = chat.reasoning_preserved.get_untracked();
    #[cfg(debug_assertions)]
    web_sys::console::log_1(
        &format!(
            "[hydration] restoring {} reasoning_text entries, aid={}",
            preserved.len(),
            aid
        )
        .into(),
    );
    if preserved.is_empty() {
        return;
    }
    chat.update_sessions_hydration(|list| {
        if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
            for m in s.messages.iter_mut() {
                if let Some(rt) = preserved.get(&m.id) {
                    #[cfg(debug_assertions)]
                    web_sys::console::log_1(
                        &format!(
                            "[hydration] restored reasoning_text len={} for mid={}",
                            rt.len(),
                            m.id
                        )
                        .into(),
                    );
                    m.reasoning_text = rt.clone();
                }
            }
        }
    });
    chat.reasoning_preserved
        .update(|map| map.retain(|k, _| !preserved.contains_key(k)));
}

fn apply_saved_revision_if_same_conversation(chat: &ChatSessionSignals, cid: &str, revision: u64) {
    chat.session_sync.update(|st| {
        if st.conversation_id.as_deref().map(str::trim) == Some(cid) {
            st.apply_saved_revision(revision);
        }
    });
}

/// 订阅 `chat.session_hydrate_nonce`：流结束后由 composer 递增，拉取服务端快照并写回当前会话。
///
/// 门闸与 [`super::app_bootstrap_phase::AppBootstrapPhase::hydration_effects_enabled`] 一致（`initialized` + `web_ui_config_loaded`）。
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
            if !AppBootstrapPhase::derive(initialized.get(), web_ui_config_loaded.get())
                .hydration_effects_enabled()
            {
                return;
            }
            let aid = chat.active_id.get();
            if aid.is_empty() {
                return;
            }
            let nonce_at_start = chat.session_hydrate_nonce.get();
            let Some(cid) = chat.sessions.with_untracked(|list| {
                list.iter().find(|s| s.id == aid).and_then(|s| {
                    if messages_contain_loading(&s.messages) {
                        return None;
                    }
                    trimmed_server_conversation_id(s).map(|c| c.to_string())
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
                chat.update_sessions_hydration(|list| {
                    let active = chat.active_id.get_untracked();
                    let cur_nonce = chat.session_hydrate_nonce.get_untracked();
                    let Some(s) = list.iter_mut().find(|x| x.id == aid) else {
                        return;
                    };
                    let merge_outcome =
                        merge_hydration_into_active_session(MergeHydrationIntoActiveSessionArgs {
                            session: s,
                            aid: &aid,
                            cid: cid.as_str(),
                            hydrated: msgs,
                            resp: &resp,
                            nonce_at_start,
                            current_nonce: cur_nonce,
                            active_id: &active,
                            selected_agent_role,
                        });
                    applied_hydration |= merge_outcome.is_applied();
                });
                if !applied_hydration {
                    return;
                }
                if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
                    return;
                }
                restore_reasoning_after_hydration(&chat, &aid, nonce_at_start);
                apply_saved_revision_if_same_conversation(&chat, cid.as_str(), resp.revision);
            });
        }
    });
}
