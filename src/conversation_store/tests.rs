use super::*;
use crate::types::{Message, message_content_as_str};

#[test]
fn truncate_before_user_ordinal_skips_first_turn_workspace_injection() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![
        Message::system_only("s".to_string()),
        Message::user_first_turn_workspace_context("ctx".to_string()),
        Message::user_only("hi".to_string()),
    ];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs, None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded.messages.len(), 3);
    assert_eq!(
        truncate_before_user_ordinal_if_revision(&conn, "c1", 0, loaded.revision).unwrap(),
        SaveConversationOutcome::Saved
    );
    let after = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(after.messages.len(), 2);
    assert_eq!(after.messages[0].role, "system");
    assert_eq!(
        message_content_as_str(&after.messages[1].content),
        Some("ctx")
    );
    assert_eq!(
        after.messages[1].name.as_deref(),
        Some(crate::types::CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME)
    );
}

#[test]
fn save_load_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![
        Message::system_only("s".to_string()),
        Message::user_only("hi".to_string()),
    ];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded.revision, 1);
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(
        loaded.layout.as_ref().map(|m| m.layout_schema_version),
        Some(2)
    );
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, Some(1)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded2 = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded2.revision, 2);
}

#[test]
fn save_load_active_agent_role_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![Message::system_only("s".to_string())];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded.active_agent_role, "");

    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), Some("reviewer"), None, Some(1)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded2 = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded2.active_agent_role, "reviewer");

    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, Some(2)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded3 = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded3.active_agent_role, "");
}

#[test]
fn save_load_active_session_mode_roundtrip() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![Message::system_only("s".to_string())];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, Some("ask"), None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded.active_session_mode, "ask");

    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, Some("plan"), Some(1)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded2 = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded2.active_session_mode, "plan");

    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, Some(2)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded3 = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded3.active_session_mode, "");
}

#[test]
fn load_optional_layout_meta_json_without_touching_save_path() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![Message::user_only("hi".to_string())];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs, None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let meta = ConversationLayoutMeta {
        layout_schema_version: crate::cm_api_contract::chat::CONVERSATION_LAYOUT_SCHEMA_VERSION_V2,
        projection_hash: Some("abc".into()),
        segments: vec![],
    };
    let json = serde_json::to_string(&meta).unwrap();
    conn.execute(
        "UPDATE crabmate_conversations SET layout_meta_json = ?1 WHERE id = ?2",
        params![json, "c1"],
    )
    .unwrap();
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert_eq!(loaded.layout.as_ref(), Some(&meta));

    conn.execute(
        "UPDATE crabmate_conversations SET layout_meta_json = ?1 WHERE id = ?2",
        params!["{not-json", "c1"],
    )
    .unwrap();
    let loaded_bad = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert!(loaded_bad.layout.is_none());
    assert_eq!(loaded_bad.messages.len(), 1);
}

#[test]
fn save_if_revision_rewrites_layout_from_messages() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![
        Message::user_only("hi".to_string()),
        Message::assistant_only("ok".to_string()),
    ];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    conn.execute(
        "UPDATE crabmate_conversations SET layout_meta_json = ?1 WHERE id = ?2",
        params![
            r#"{"layout_schema_version":2,"projection_hash":"stale"}"#,
            "c1"
        ],
    )
    .unwrap();
    assert_eq!(
        save_if_revision(&conn, "c1", msgs.clone(), None, None, Some(1)).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    let expected = crate::cm_turn_layout::layout_meta_from_messages(&msgs);
    assert_eq!(loaded.layout.as_ref(), Some(&expected));
    assert_ne!(
        loaded
            .layout
            .as_ref()
            .and_then(|m| m.projection_hash.as_deref()),
        Some("stale")
    );
}

#[test]
fn truncate_before_user_rewrites_layout_meta() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let msgs = vec![
        Message::user_only("hi".to_string()),
        Message::assistant_only("ok".to_string()),
    ];
    assert_eq!(
        save_if_revision(&conn, "c1", msgs, None, None, None).unwrap(),
        SaveConversationOutcome::Saved
    );
    let loaded = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert!(!loaded.layout.as_ref().unwrap().segments.is_empty());
    assert_eq!(
        truncate_before_user_ordinal_if_revision(&conn, "c1", 0, loaded.revision).unwrap(),
        SaveConversationOutcome::Saved
    );
    let after = load(&conn, "c1", 3600).unwrap().expect("exists");
    assert!(after.messages.is_empty());
    assert!(after.layout.as_ref().unwrap().segments.is_empty());
}

fn seed_ids(conn: &Connection, ids: &[&str]) {
    for id in ids {
        assert_eq!(
            save_if_revision(
                conn,
                id,
                vec![Message::user_only("hi".to_string())],
                None,
                None,
                None,
            )
            .unwrap(),
            SaveConversationOutcome::Saved
        );
    }
}

fn backdate(conn: &Connection, id: &str, updated_at_unix: i64) {
    conn.execute(
        "UPDATE crabmate_conversations SET updated_at_unix = ?1 WHERE id = ?2",
        params![updated_at_unix, id],
    )
    .unwrap();
}

#[test]
fn prune_removes_rows_older_than_ttl() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    seed_ids(&conn, &["old", "fresh"]);
    backdate(&conn, "old", now_unix() - 10_000);
    prune(&conn, 3600, 0).unwrap();
    assert_eq!(count(&conn).unwrap(), 1);
    assert!(load(&conn, "old", 0).unwrap().is_none());
    assert!(load(&conn, "fresh", 0).unwrap().is_some());
}

#[test]
fn prune_ttl_zero_keeps_expired_rows() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    seed_ids(&conn, &["ancient", "fresh"]);
    backdate(&conn, "ancient", 0);
    prune(&conn, 0, 0).unwrap();
    assert_eq!(count(&conn).unwrap(), 2);
}

#[test]
fn prune_max_entries_zero_keeps_all_rows() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    seed_ids(&conn, &["a", "b", "c"]);
    prune(&conn, 0, 0).unwrap();
    assert_eq!(count(&conn).unwrap(), 3);
}

#[test]
fn prune_evicts_oldest_beyond_max_entries() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    seed_ids(&conn, &["a", "b", "c"]);
    let now = now_unix();
    backdate(&conn, "a", now - 300);
    backdate(&conn, "b", now - 200);
    backdate(&conn, "c", now - 100);
    prune(&conn, 0, 2).unwrap();
    assert_eq!(count(&conn).unwrap(), 2);
    assert!(load(&conn, "a", 0).unwrap().is_none(), "oldest evicted");
    assert!(load(&conn, "b", 0).unwrap().is_some());
    assert!(load(&conn, "c", 0).unwrap().is_some());
}

#[test]
fn delete_by_id_removes_row_and_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    seed_ids(&conn, &["c1"]);
    delete_by_id(&conn, "c1").unwrap();
    assert_eq!(count(&conn).unwrap(), 0);
    delete_by_id(&conn, "c1").unwrap();
    delete_by_id(&conn, "never-existed").unwrap();
}

fn arow(reason: ArchiveReason, start: i64, end: i64, text: &str) -> ArchiveRow {
    ArchiveRow {
        reason,
        range_start: start,
        range_end: end,
        messages: vec![Message::user_only(text.to_string())],
    }
}

fn archive_codecs(conn: &Connection, id: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT codec FROM {ARCHIVE_TABLE} WHERE conversation_id = ?1 ORDER BY seq ASC"
        ))
        .unwrap();
    stmt.query_map(params![id], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn archive_roundtrip_identity_and_gzip() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let small = vec![Message::user_only("hi".to_string())];
    let big: Vec<Message> = (0..200)
        .map(|i| Message::assistant_only(format!("chunk-{i}-{}", "x".repeat(64))))
        .collect();
    append_archive_rows(
        &conn,
        "c1",
        &[
            ArchiveRow {
                reason: ArchiveReason::Summarized,
                range_start: 0,
                range_end: 1,
                messages: small.clone(),
            },
            ArchiveRow {
                reason: ArchiveReason::RemovedTurnGroup,
                range_start: 1,
                range_end: 201,
                messages: big.clone(),
            },
        ],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();

    assert_eq!(
        archive_codecs(&conn, "c1"),
        vec!["identity".to_string(), "gzip".to_string()]
    );

    let loaded = load_archive_messages(&conn, "c1").unwrap();
    assert_eq!(loaded.len(), 201);
    assert_eq!(message_content_as_str(&loaded[0].content), Some("hi"));
    assert_eq!(
        message_content_as_str(&loaded[1].content),
        message_content_as_str(&big[0].content)
    );
}

#[test]
fn archive_force_gzip_and_force_identity() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    // 阈值 1：任何非空 payload 都走 gzip。
    append_archive_rows(
        &conn,
        "gz",
        &[arow(ArchiveReason::Summarized, 0, 1, "tiny")],
        1,
    )
    .unwrap();
    // 阈值 0：显式禁用 gzip（始终 identity）。
    append_archive_rows(
        &conn,
        "id",
        &[arow(ArchiveReason::Summarized, 0, 1, "tiny")],
        0,
    )
    .unwrap();
    assert_eq!(archive_codecs(&conn, "gz"), vec!["gzip".to_string()]);
    assert_eq!(archive_codecs(&conn, "id"), vec!["identity".to_string()]);
    assert_eq!(load_archive_messages(&conn, "gz").unwrap().len(), 1);
    assert_eq!(load_archive_messages(&conn, "id").unwrap().len(), 1);
}

#[test]
fn archive_seq_is_monotonic_across_appends() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    append_archive_rows(
        &conn,
        "c1",
        &[arow(ArchiveReason::Summarized, 0, 1, "a")],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    append_archive_rows(
        &conn,
        "c1",
        &[
            arow(ArchiveReason::Artifact, 1, 2, "b"),
            arow(ArchiveReason::Artifact, 2, 3, "c"),
        ],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    let seqs: Vec<i64> = {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT seq FROM {ARCHIVE_TABLE} WHERE conversation_id = 'c1' ORDER BY seq ASC"
            ))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(seqs, vec![0, 1, 2]);
    // 会话隔离：别的会话互不影响。
    assert!(load_archive_messages(&conn, "other").unwrap().is_empty());
}

#[test]
fn archive_load_surfaces_decode_error_not_silent() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    append_archive_rows(
        &conn,
        "c1",
        &[arow(ArchiveReason::Summarized, 0, 1, "x")],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    // 把 identity 行谎称 gzip：base64 解码失败 → 必须报错而非静默丢弃。
    conn.execute(
        &format!("UPDATE {ARCHIVE_TABLE} SET codec = 'gzip' WHERE conversation_id = 'c1'"),
        [],
    )
    .unwrap();
    let err = load_archive_messages(&conn, "c1").unwrap_err();
    assert!(matches!(err, ArchiveError::Decode { .. }), "got {err:?}");

    conn.execute(
        &format!("UPDATE {ARCHIVE_TABLE} SET codec = 'bogus' WHERE conversation_id = 'c1'"),
        [],
    )
    .unwrap();
    assert!(matches!(
        load_archive_messages(&conn, "c1").unwrap_err(),
        ArchiveError::Decode { .. }
    ));
}

#[test]
fn delete_archive_for_conversation_removes_only_that_conversation() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    append_archive_rows(
        &conn,
        "c1",
        &[arow(ArchiveReason::Summarized, 0, 1, "a")],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    append_archive_rows(
        &conn,
        "c2",
        &[arow(ArchiveReason::Summarized, 0, 1, "b")],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    delete_archive_for_conversation(&conn, "c1").unwrap();
    assert!(load_archive_messages(&conn, "c1").unwrap().is_empty());
    assert_eq!(load_archive_messages(&conn, "c2").unwrap().len(), 1);
}

#[test]
fn prune_archive_ttl_and_per_conversation_cap() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    append_archive_rows(
        &conn,
        "c1",
        &[
            arow(ArchiveReason::Summarized, 0, 1, "a"),
            arow(ArchiveReason::Summarized, 1, 2, "b"),
            arow(ArchiveReason::Summarized, 2, 3, "c"),
        ],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    prune_archive(&conn, 0, 2).unwrap();
    let kept = load_archive_messages(&conn, "c1").unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(message_content_as_str(&kept[0].content), Some("b"));
    assert_eq!(message_content_as_str(&kept[1].content), Some("c"));

    append_archive_rows(
        &conn,
        "c2",
        &[arow(ArchiveReason::Summarized, 0, 1, "old")],
        ARCHIVE_CODEC_MIN_BYTES_DEFAULT,
    )
    .unwrap();
    conn.execute(
        &format!("UPDATE {ARCHIVE_TABLE} SET created_at_unix = ?1 WHERE conversation_id = 'c2'"),
        params![now_unix() - 10_000],
    )
    .unwrap();
    prune_archive(&conn, 3600, 0).unwrap();
    assert!(load_archive_messages(&conn, "c2").unwrap().is_empty());
    assert_eq!(load_archive_messages(&conn, "c1").unwrap().len(), 2);
}
