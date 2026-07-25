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
