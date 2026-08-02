use crate::app::chat::session_hydrate::{
    apply_hydrated_tail_if_newer, hydration_response_meta_is_fresh,
    hydration_revision_after_response, merge_tail_page_into_session_messages,
    should_merge_hydrated_messages,
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
        active_session_mode: None,
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
fn hydration_meta_fresh_when_response_not_older() {
    assert!(hydration_response_meta_is_fresh(None, 1));
    assert!(hydration_response_meta_is_fresh(Some(7), 7));
    assert!(hydration_response_meta_is_fresh(Some(7), 8));
    assert!(!hydration_response_meta_is_fresh(Some(9), 7));
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
fn newer_same_turn_hydration_keeps_v2_projection_without_legacy_pool_merge() {
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
fn newer_hydration_appends_server_only_turn_after_v2_projection() {
    let local = vec![
        plain_message("u-local", "user", "question"),
        plain_message("turn-final-answer", "assistant", "本地终答"),
    ];
    let mut session = revision_session(local, Some(7));
    session.layout_schema_version = CURRENT_LAYOUT_SCHEMA_VERSION;
    let hydrated = vec![
        plain_message("h-user-1", "user", "question"),
        plain_message("h-answer-1", "assistant", "服务端旧终答"),
        plain_message("h-user-2", "user", "new question"),
        plain_message("h-answer-2", "assistant", "服务端新增终答"),
    ];

    apply_hydrated_tail_if_newer(&mut session, hydrated, &revision_response(8));

    assert_eq!(
        session
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        ["u-local", "turn-final-answer", "h-user-2", "h-answer-2"]
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
fn merge_tail_page_drops_intent_before_server_answer() {
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
        active_session_mode: None,
        tiktoken_prompt_tokens: None,
    };
    let merged = merge_tail_page_into_session_messages(&session, hydrated, &resp);
    let ids: Vec<_> = merged.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["u1", "a-srv"]);
}

#[test]
fn merge_tail_page_keeps_user_when_server_omits_user_and_drops_intent() {
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
        active_session_mode: None,
        tiktoken_prompt_tokens: None,
    };
    let merged = merge_tail_page_into_session_messages(&session, hydrated, &resp);
    let roles: Vec<_> = merged.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, vec!["user", "assistant"]);
    assert_eq!(merged[0].text, "你好");
    assert!(!merged.iter().any(|m| m.text.contains("意图分析")));
}
