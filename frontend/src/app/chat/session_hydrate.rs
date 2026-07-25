//! `GET /conversation/messages` 与本地 [`crate::storage::ChatSession`] 对齐（水合）。
//!
//! 位于 **`app/chat/`**，与 [`super::wire_chat_session_lifecycle`] 顺序接线。
//!
//! ## Effect 订阅纪律
//!
//! - **只订阅** `session_hydrate_nonce` + `active_id`（及门闸 `AppBootstrapPhase::hydration_effects_enabled`）。
//! - **勿**订阅 `sessions` 或会被水合写回的信号，否则会在合并后再次触发并叠加重复行。
//! - 异步段经 [`conversation_hydration_cycle::run`]；同步段用 [`try_hydration_wire_snapshot`]。
//!
//! ## 尾部合并
//!
//! - v2 finalized projection 保持不可变；空缓存或 v1 会话才调用
//!   [`super::session_merge::merge_session_tail`] 执行 plain user 补回与 local 顺序回放。

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app::app_bootstrap_phase::AppBootstrapPhase;
use crate::app::status_tasks_state::StatusTasksSignals;
use crate::app_prefs::status_bar_selected_agent_role_from_persisted;
use crate::chat_session_state::ChatSessionSignals;
use crate::conversation_hydrate::{
    ConversationMessagesResponse, stored_messages_from_conversation_api,
};
use crate::i18n::{self, Locale};
use crate::message_loading::messages_have_any_loading;
use crate::session_ops::title_from_user_prompt;
use crate::storage::{ChatSession, LEGACY_LAYOUT_SCHEMA_VERSION, StoredMessage};

use super::session_merge::merge_session_tail;

fn count_user_role_bubbles(messages: &[StoredMessage]) -> usize {
    messages.iter().filter(|m| m.role == "user").count()
}

fn conversation_server_id_if_hydratable_for_wire(s: &ChatSession) -> Option<String> {
    if messages_have_any_loading(&s.messages) {
        return None;
    }
    s.trimmed_server_conversation_id().map(str::to_string)
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
    agent_role_user_override: RwSignal<bool>,
    default_agent_role_id: Option<&'a str>,
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
    if messages_have_any_loading(&session.messages) {
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
fn apply_history_meta_from_response(
    session: &mut ChatSession,
    resp: &ConversationMessagesResponse,
) {
    if resp.total_count > 0 || !resp.messages.is_empty() {
        session.history_total = Some(resp.total_count);
        session.history_window_start = Some(resp.window_start_index);
        session.history_has_older = Some(resp.has_older);
    }
}

/// 尾部水合：保留已加载的更早前缀，仅替换与服务器尾部重叠段。
fn merge_tail_page_into_session_messages(
    session: &ChatSession,
    hydrated: Vec<StoredMessage>,
    resp: &ConversationMessagesResponse,
) -> Vec<StoredMessage> {
    if session.has_v2_layout_projection() && session.has_v2_finalized_rows() {
        return session.messages.clone();
    }
    let tail_start = resp.window_start_index;
    if let Some(local_start) = session.history_window_start {
        if tail_start >= local_start {
            let keep = (tail_start - local_start) as usize;
            let keep = keep.min(session.messages.len());
            let local_tail = &session.messages[keep..];
            let mut out: Vec<StoredMessage> = session.messages[..keep].to_vec();
            out.extend(merge_session_tail(hydrated, local_tail));
            return out;
        }
    }
    merge_session_tail(hydrated, session.messages.as_slice())
}

fn should_merge_hydrated_messages(
    session: &ChatSession,
    resp: &ConversationMessagesResponse,
) -> bool {
    session.messages.is_empty()
        || session
            .server_revision
            .is_none_or(|local_revision| local_revision < resp.revision)
}

fn hydration_revision_after_response(local_revision: Option<u64>, response_revision: u64) -> u64 {
    local_revision.unwrap_or_default().max(response_revision)
}

fn apply_hydrated_tail_if_newer(
    session: &mut ChatSession,
    hydrated: Vec<StoredMessage>,
    resp: &ConversationMessagesResponse,
) {
    if !should_merge_hydrated_messages(session, resp) {
        return;
    }
    let preserved_v2_projection =
        session.has_v2_layout_projection() && session.has_v2_finalized_rows();
    session.messages = merge_tail_page_into_session_messages(session, hydrated, resp);
    if !preserved_v2_projection {
        // `/conversation/messages` 暂无 segment projection key；空缓存或 v1 缓存走 legacy adapter。
        session.layout_schema_version = LEGACY_LAYOUT_SCHEMA_VERSION;
    }
}

fn prepend_older_page_into_session(
    session: &mut ChatSession,
    hydrated: Vec<StoredMessage>,
    resp: &ConversationMessagesResponse,
) {
    let existing_ids: HashSet<_> = session.messages.iter().map(|m| m.id.as_str()).collect();
    let older: Vec<StoredMessage> = hydrated
        .into_iter()
        .filter(|m| !existing_ids.contains(m.id.as_str()))
        .collect();
    let mut combined = older;
    combined.append(&mut session.messages);
    session.messages = combined;
    apply_history_meta_from_response(session, resp);
}

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
        agent_role_user_override,
        default_agent_role_id,
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
    apply_hydrated_tail_if_newer(session, hydrated, resp);
    apply_history_meta_from_response(session, resp);
    session.server_revision = Some(hydration_revision_after_response(
        session.server_revision,
        resp.revision,
    ));
    if !agent_role_user_override.get_untracked()
        && let Some(role) = resp
            .active_agent_role
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
    {
        selected_agent_role.set(status_bar_selected_agent_role_from_persisted(
            Some(role),
            default_agent_role_id,
        ));
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
    if chat.defers_conversation_hydration_untracked() {
        return None;
    }
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
        agent_role_user_override: RwSignal<bool>,
        default_agent_role_id: Option<String>,
    ) {
        let HydrationWireSnapshot {
            aid,
            cid,
            nonce_at_start,
            locale,
        } = snap;
        let Ok(resp) = fetch_conversation_messages(
            &cid,
            crate::conversation_messages_page::ConversationMessagesFetchParams::tail_page(),
            locale,
        )
        .await
        else {
            return;
        };

        if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
            return;
        }
        if chat.defers_conversation_hydration_untracked() {
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
            if chat.defers_conversation_hydration_untracked() {
                return;
            }
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
                    agent_role_user_override,
                    default_agent_role_id: default_agent_role_id.as_deref(),
                });
            applied_hydration |= merge_outcome.is_applied();
        });

        if !applied_hydration {
            return;
        }
        if chat.session_hydrate_nonce.get_untracked() != nonce_at_start {
            return;
        }

        if let Some(snap) = resp.tiktoken_prompt_tokens.clone() {
            crate::conversation_prompt_tokens_apply::apply_conversation_prompt_tokens_from_sse(
                chat,
                cid.as_str(),
                snap,
            );
        } else {
            chat.conversation_prompt_tokens
                .set(Some(ConversationPromptTokenHydrate {
                    conversation_id: cid.clone(),
                    tiktoken: None,
                }));
        }

        restore_reasoning_after_hydration(&chat, &aid, nonce_at_start);
        apply_saved_revision_if_same_conversation(&chat, cid.as_str(), resp.revision);
    }
}

async fn run_conversation_hydration_cycle(
    snap: HydrationWireSnapshot,
    chat: ChatSessionSignals,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    default_agent_role_id: Option<String>,
) {
    let _stream_lane = chat.stream_lane_overlay_phase_untracked();
    conversation_hydration_cycle::run(
        snap,
        chat,
        selected_agent_role,
        agent_role_user_override,
        default_agent_role_id,
    )
    .await;
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
    if messages_have_any_loading(&sess.messages) {
        return;
    }
    if sess.trimmed_server_conversation_id().is_none() {
        chat.conversation_prompt_tokens.set(None);
    }
}

/// 滚动到顶附近时由 UI 按钮拉取更早一页（须已绑定 `server_conversation_id` 且 `history_has_older`）。
pub(crate) fn try_load_older_messages_for_active_session(
    chat: ChatSessionSignals,
    locale: Locale,
    scroll_shell: super::scroll_shell::ChatScrollShellSignals,
) {
    if chat.history_loading_older.get_untracked() {
        return;
    }
    let Some(snap) = try_hydration_wire_snapshot(chat, locale) else {
        return;
    };
    let Some(window_start) = chat.sessions.with_untracked(|list| {
        list.iter()
            .find(|s| s.id == snap.aid)
            .and_then(|s| s.history_window_start)
    }) else {
        return;
    };
    let has_older = chat.sessions.with_untracked(|list| {
        list.iter()
            .find(|s| s.id == snap.aid)
            .is_some_and(|s| s.history_has_older_flag())
    });
    if !has_older {
        return;
    }
    chat.history_loading_older.set(true);
    let prepend_snap = scroll_shell.capture_prepend_snapshot();
    let chat2 = chat;
    spawn_local(async move {
        let Ok(resp) = crate::api::fetch_conversation_messages(
            &snap.cid,
            crate::conversation_messages_page::ConversationMessagesFetchParams::older_before(
                window_start,
            ),
            snap.locale,
        )
        .await
        else {
            chat2.history_loading_older.set(false);
            return;
        };
        if chat2.session_hydrate_nonce.get_untracked() != snap.nonce_at_start {
            chat2.history_loading_older.set(false);
            return;
        }
        let msgs = stored_messages_from_conversation_api(&resp.messages);
        chat2.update_sessions_hydration(|list| {
            let Some(s) = list.iter_mut().find(|x| x.id == snap.aid) else {
                return;
            };
            if s.trimmed_server_conversation_id() != Some(snap.cid.as_str()) {
                return;
            }
            prepend_older_page_into_session(s, msgs, &resp);
        });
        scroll_shell.compensate_after_prepend(prepend_snap);
        chat2.history_loading_older.set(false);
    });
}

/// 递增水合触发计数（会话列表就绪、流式收尾等），驱动下方 Effect 拉取 `GET /conversation/messages`。
pub(crate) fn bump_session_hydrate_nonce(chat: ChatSessionSignals) {
    chat.session_hydrate_nonce
        .update(|n| *n = n.wrapping_add(1));
}

/// 订阅 `session_hydrate_nonce` 与 `active_id`：拉取服务端快照并写回当前会话（含 tiktoken 用量）。
///
/// **勿**订阅 `sessions`：水合写回会更新消息列表，若再触发本 Effect 会在每轮生成新 `h_*` id 并重复追加工具行。
///
/// 门闸与 [`crate::app::app_bootstrap_phase::AppBootstrapPhase::hydration_effects_enabled`] 一致（`initialized` + `web_ui_config_loaded`）。
pub fn wire_session_hydration(
    initialized: RwSignal<bool>,
    web_ui_config_loaded: RwSignal<bool>,
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    selected_agent_role: RwSignal<Option<String>>,
    agent_role_user_override: RwSignal<bool>,
    status_tasks: StatusTasksSignals,
) {
    Effect::new({
        let chat = chat;
        let locale_sig = locale;
        let selected_agent_role = selected_agent_role;
        let agent_role_user_override = agent_role_user_override;
        let status_tasks = status_tasks;
        move |_| {
            if !AppBootstrapPhase::derive(initialized.get(), web_ui_config_loaded.get())
                .hydration_effects_enabled()
            {
                return;
            }
            let _ = chat.active_id.get();
            let _ = chat.session_hydrate_nonce.get();
            let loc = locale_sig.get_untracked();
            let default_agent_role_id = status_tasks
                .status_data
                .get_untracked()
                .and_then(|d| d.default_agent_role_id.clone());
            let Some(snap) = try_hydration_wire_snapshot(chat, loc) else {
                clear_conversation_prompt_tokens_if_no_server_conversation(chat);
                return;
            };
            spawn_local(run_conversation_hydration_cycle(
                snap,
                chat,
                selected_agent_role,
                agent_role_user_override,
                default_agent_role_id,
            ));
        }
    });
}

#[cfg(test)]
mod merge_tail_page_order_tests {
    use super::{
        apply_hydrated_tail_if_newer, hydration_revision_after_response,
        merge_tail_page_into_session_messages, should_merge_hydrated_messages,
    };
    use crate::conversation_hydrate::ConversationMessagesResponse;
    use crate::storage::{
        CURRENT_LAYOUT_SCHEMA_VERSION, ChatSession, LEGACY_LAYOUT_SCHEMA_VERSION, StoredMessage,
    };
    use crate::timeline_scan::timeline_state_intent_analysis_snapshot;

    fn revision_session(messages: Vec<StoredMessage>, revision: Option<u64>) -> ChatSession {
        ChatSession {
            id: "sid".into(),
            layout_schema_version: LEGACY_LAYOUT_SCHEMA_VERSION,
            title: "t".into(),
            draft: String::new(),
            messages,
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: Some("cid".into()),
            server_revision: revision,
            workspace_root: None,
            history_total: None,
            history_window_start: Some(0),
            history_has_older: None,
        }
    }

    fn revision_response(revision: u64) -> ConversationMessagesResponse {
        ConversationMessagesResponse {
            conversation_id: "cid".into(),
            messages: vec![],
            revision,
            total_count: 1,
            window_start_index: 0,
            has_older: false,
            active_agent_role: None,
            tiktoken_prompt_tokens: None,
        }
    }

    fn plain_message(id: &str, role: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn same_revision_keeps_nonempty_local_projection() {
        let local = plain_message("turn-commentary-tc_read", "assistant", "已关闭的本地旁注");
        let session = revision_session(vec![local], Some(7));
        assert!(!should_merge_hydrated_messages(
            &session,
            &revision_response(7)
        ));
        assert!(!should_merge_hydrated_messages(
            &session,
            &revision_response(6)
        ));
        assert!(should_merge_hydrated_messages(
            &session,
            &revision_response(8)
        ));
        assert!(should_merge_hydrated_messages(
            &revision_session(vec![], Some(7)),
            &revision_response(7)
        ));
    }

    #[test]
    fn stale_hydration_response_neither_merges_nor_downgrades_revision() {
        let local = plain_message("turn-final-answer", "assistant", "本地较新终答");
        let session = revision_session(vec![local], Some(9));
        let stale_response = revision_response(7);
        assert!(!should_merge_hydrated_messages(&session, &stale_response));
        assert_eq!(
            hydration_revision_after_response(session.server_revision, stale_response.revision),
            9
        );
    }

    #[test]
    fn newer_hydration_keeps_nonempty_v2_projection_without_legacy_pool_merge() {
        let local = vec![
            plain_message("u-local", "user", "question"),
            plain_message("turn-commentary-tc_read", "assistant", "不可变 commentary"),
            plain_message("turn-final-answer", "assistant", "本地终答"),
        ];
        let mut session = revision_session(local.clone(), Some(7));
        session.layout_schema_version = CURRENT_LAYOUT_SCHEMA_VERSION;
        let hydrated = vec![
            plain_message("h-user", "user", "question"),
            plain_message("h-assistant", "assistant", "服务端 canonical 快照"),
        ];

        apply_hydrated_tail_if_newer(&mut session, hydrated, &revision_response(8));

        assert_eq!(
            session
                .messages
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            local.iter().map(|m| m.id.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(session.layout_schema_version, CURRENT_LAYOUT_SCHEMA_VERSION);
    }

    #[test]
    fn empty_v2_cache_uses_legacy_adapter_for_canonical_hydration() {
        let mut session = revision_session(vec![], Some(7));
        session.layout_schema_version = CURRENT_LAYOUT_SCHEMA_VERSION;
        let hydrated = vec![plain_message("h-assistant", "assistant", "服务端终答")];

        apply_hydrated_tail_if_newer(&mut session, hydrated, &revision_response(7));

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].id, "h-assistant");
        assert_eq!(session.layout_schema_version, LEGACY_LAYOUT_SCHEMA_VERSION);
    }

    #[test]
    fn v1_history_still_uses_legacy_adapter() {
        let mut session = revision_session(
            vec![plain_message("local-assistant", "assistant", "本地旧投影")],
            Some(1),
        );
        let hydrated = vec![plain_message("h-assistant", "assistant", "服务端旧会话")];

        apply_hydrated_tail_if_newer(&mut session, hydrated, &revision_response(2));

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].id, "h-assistant");
        assert_eq!(session.layout_schema_version, LEGACY_LAYOUT_SCHEMA_VERSION);
    }

    #[test]
    fn merge_tail_page_keeps_intent_before_server_answer() {
        let session = ChatSession {
            id: "sid".into(),
            layout_schema_version: LEGACY_LAYOUT_SCHEMA_VERSION,
            title: "t".into(),
            draft: String::new(),
            messages: vec![
                plain_message("u1", "user", "question"),
                StoredMessage {
                    state: Some(timeline_state_intent_analysis_snapshot()),
                    created_at: 1,
                    ..plain_message("tl-intent", "assistant", "意图分析：执行类\n\n")
                },
                StoredMessage {
                    created_at: 2,
                    ..plain_message("a-local", "assistant", "stream draft")
                },
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: Some("cid".into()),
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: Some(0),
            history_has_older: None,
        };
        let hydrated = vec![
            plain_message("u1", "user", "question"),
            StoredMessage {
                created_at: 2,
                ..plain_message("a-srv", "assistant", "final answer")
            },
        ];
        let resp = ConversationMessagesResponse {
            conversation_id: "cid".into(),
            messages: vec![],
            revision: 1,
            total_count: 2,
            window_start_index: 0,
            has_older: false,
            active_agent_role: None,
            tiktoken_prompt_tokens: None,
        };
        let merged = merge_tail_page_into_session_messages(&session, hydrated, &resp);
        let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["u1", "tl-intent", "a-srv"],
            "intent should precede canonical answer"
        );
    }

    #[test]
    fn merge_tail_page_keeps_user_before_intent_when_server_omits_user() {
        let session = ChatSession {
            id: "sid".into(),
            layout_schema_version: LEGACY_LAYOUT_SCHEMA_VERSION,
            title: "t".into(),
            draft: String::new(),
            messages: vec![
                plain_message("u-local", "user", "你好"),
                StoredMessage {
                    state: Some(timeline_state_intent_analysis_snapshot()),
                    created_at: 1,
                    ..plain_message("tl-intent", "assistant", "意图分析：问候类\n\n")
                },
                StoredMessage {
                    created_at: 2,
                    ..plain_message("a-local", "assistant", "你好！")
                },
            ],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: Some("cid".into()),
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: Some(0),
            history_has_older: None,
        };
        let hydrated = vec![
            StoredMessage {
                state: Some(timeline_state_intent_analysis_snapshot()),
                created_at: 1,
                ..plain_message("tl-intent-srv", "assistant", "意图分析：问候类\n\n")
            },
            StoredMessage {
                created_at: 2,
                ..plain_message("a-srv", "assistant", "你好！我是 CrabMate 的 AI 助手。")
            },
        ];
        let resp = ConversationMessagesResponse {
            conversation_id: "cid".into(),
            messages: vec![],
            revision: 1,
            total_count: 2,
            window_start_index: 0,
            has_older: false,
            active_agent_role: None,
            tiktoken_prompt_tokens: None,
        };
        let merged = merge_tail_page_into_session_messages(&session, hydrated, &resp);
        let roles: Vec<_> = merged.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "assistant"]);
        assert_eq!(merged[0].text, "你好");
    }
}

#[cfg(test)]
mod conversation_server_id_for_hydrate_tests {
    use super::conversation_server_id_if_hydratable_for_wire;
    use crate::storage::{
        ChatSession, LEGACY_LAYOUT_SCHEMA_VERSION, StoredMessage, StoredMessageState,
    };

    fn base_session() -> ChatSession {
        ChatSession {
            id: "sid".into(),
            layout_schema_version: LEGACY_LAYOUT_SCHEMA_VERSION,
            title: "t".into(),
            draft: String::new(),
            messages: vec![],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: Some("  srv-9  ".into()),
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
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
