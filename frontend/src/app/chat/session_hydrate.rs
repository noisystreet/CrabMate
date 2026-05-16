//! `GET /conversation/messages` 与本地 [`crate::storage::ChatSession`] 对齐（水合）。
//!
//! 位于 **`app/chat/`**，与 [`super::wire_chat_session_lifecycle`] 顺序接线；Effect **同步段**用 [`try_hydration_wire_snapshot`] 生成 [`HydrationWireSnapshot`]，再 `spawn_local` 进入 [`conversation_hydration_cycle::run`]（经 [`run_conversation_hydration_cycle`] 薄包装），与流式写入 nonce 门闩对齐。

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app::app_bootstrap_phase::AppBootstrapPhase;
use crate::chat_session_state::ChatSessionSignals;
use crate::conversation_hydrate::ConversationMessagesResponse;
use crate::i18n::{self, Locale};
use crate::session_ops::title_from_user_prompt;
use crate::storage::{ChatSession, StoredMessage};

fn count_user_role_bubbles(messages: &[StoredMessage]) -> usize {
    messages.iter().filter(|m| m.role == "user").count()
}

fn messages_contain_loading(messages: &[StoredMessage]) -> bool {
    messages
        .iter()
        .any(|m| m.state.as_ref().is_some_and(|s| s.is_loading()))
}

fn conversation_server_id_if_hydratable_for_wire(s: &ChatSession) -> Option<String> {
    if messages_contain_loading(&s.messages) {
        return None;
    }
    s.trimmed_server_conversation_id().map(str::to_string)
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
    let still = session.trimmed_server_conversation_id();
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

/// [`wire_session_hydration`] 的 Effect **同步段**解析结果：进入 `spawn_local` 后只读此快照与信号句柄，避免与响应式订阅交错。
pub(crate) struct HydrationWireSnapshot {
    aid: String,
    cid: String,
    nonce_at_start: u64,
    locale: Locale,
}

fn try_hydration_wire_snapshot(
    chat: ChatSessionSignals,
    locale: Locale,
) -> Option<HydrationWireSnapshot> {
    let aid = chat.active_id.get();
    if aid.is_empty() {
        return None;
    }
    let nonce_at_start = chat.session_hydrate_nonce.get();
    let cid = chat.sessions.with_untracked(|list| {
        list.iter()
            .find(|s| s.id == aid)
            .and_then(conversation_server_id_if_hydratable_for_wire)
    })?;
    Some(HydrationWireSnapshot {
        aid,
        cid,
        nonce_at_start,
        locale,
    })
}

/// 将 [`run_conversation_hydration_cycle`] 主体收拢为可单测对照的 FSM 式模块。
pub(crate) mod conversation_hydration_cycle {
    use leptos::prelude::*;

    use crate::api::fetch_conversation_messages;
    use crate::chat_session_state::{ChatSessionSignals, ConversationPromptTokenHydrate};
    use crate::conversation_hydrate::stored_messages_from_conversation_api;

    use super::{
        HydrationWireSnapshot, MergeHydrationIntoActiveSessionArgs,
        apply_saved_revision_if_same_conversation, merge_hydration_into_active_session,
        restore_reasoning_after_hydration,
    };

    pub(crate) async fn run(
        snap: HydrationWireSnapshot,
        chat: ChatSessionSignals,
        selected_agent_role: RwSignal<Option<String>>,
    ) {
        let HydrationWireSnapshot {
            aid,
            cid,
            nonce_at_start,
            locale,
        } = snap;
        let Ok(resp) = fetch_conversation_messages(&cid, locale).await else {
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

        chat.conversation_prompt_tokens
            .set(Some(ConversationPromptTokenHydrate {
                conversation_id: cid.clone(),
                tiktoken: resp.tiktoken_prompt_tokens.clone(),
            }));

        restore_reasoning_after_hydration(&chat, &aid, nonce_at_start);
        apply_saved_revision_if_same_conversation(&chat, cid.as_str(), resp.revision);
    }
}

async fn run_conversation_hydration_cycle(
    snap: HydrationWireSnapshot,
    chat: ChatSessionSignals,
    selected_agent_role: RwSignal<Option<String>>,
) {
    let _stream_lane = chat.stream_lane_overlay_phase_untracked();
    conversation_hydration_cycle::run(snap, chat, selected_agent_role).await;
}

fn clear_conversation_prompt_tokens_if_no_server_conversation(chat: ChatSessionSignals) {
    let aid = chat.active_id.get_untracked();
    if aid.is_empty() {
        chat.conversation_prompt_tokens.set(None);
        return;
    }
    let Some(sess) = chat
        .sessions
        .with_untracked(|list| list.iter().find(|s| s.id == aid).cloned())
    else {
        chat.conversation_prompt_tokens.set(None);
        return;
    };
    if messages_contain_loading(&sess.messages) {
        return;
    }
    if sess.trimmed_server_conversation_id().is_none() {
        chat.conversation_prompt_tokens.set(None);
    }
}

/// 订阅 `chat.session_hydrate_nonce`：流结束后由 composer 递增，拉取服务端快照并写回当前会话。
///
/// 门闸与 [`crate::app::app_bootstrap_phase::AppBootstrapPhase::hydration_effects_enabled`] 一致（`initialized` + `web_ui_config_loaded`）。
pub fn wire_session_hydration(
    initialized: RwSignal<bool>,
    web_ui_config_loaded: RwSignal<bool>,
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    selected_agent_role: RwSignal<Option<String>>,
) {
    Effect::new({
        let chat = chat;
        let locale_sig = locale;
        let selected_agent_role = selected_agent_role;
        move |_| {
            if !AppBootstrapPhase::derive(initialized.get(), web_ui_config_loaded.get())
                .hydration_effects_enabled()
            {
                return;
            }
            let loc = locale_sig.get_untracked();
            let Some(snap) = try_hydration_wire_snapshot(chat, loc) else {
                clear_conversation_prompt_tokens_if_no_server_conversation(chat);
                return;
            };
            spawn_local(run_conversation_hydration_cycle(
                snap,
                chat,
                selected_agent_role,
            ));
        }
    });
}

#[cfg(test)]
mod conversation_server_id_for_hydrate_tests {
    use super::conversation_server_id_if_hydratable_for_wire;
    use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

    fn base_session() -> ChatSession {
        ChatSession {
            id: "sid".into(),
            title: "t".into(),
            draft: String::new(),
            messages: vec![],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: Some("  srv-9  ".into()),
            server_revision: None,
            workspace_root: None,
        }
    }

    #[test]
    fn returns_trimmed_id_without_loading() {
        let s = base_session();
        assert_eq!(
            conversation_server_id_if_hydratable_for_wire(&s).as_deref(),
            Some("srv-9")
        );
    }

    #[test]
    fn returns_none_when_any_loading_placeholder() {
        let mut s = base_session();
        s.messages.push(StoredMessage {
            id: "m1".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        });
        assert!(conversation_server_id_if_hydratable_for_wire(&s).is_none());
    }

    #[test]
    fn returns_none_without_server_conversation_id() {
        let mut s = base_session();
        s.server_conversation_id = None;
        assert!(conversation_server_id_if_hydratable_for_wire(&s).is_none());
    }
}
