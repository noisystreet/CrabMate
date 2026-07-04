use super::*;
use crate::storage::{StoredMessage, StoredMessageState};

fn empty_msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
    StoredMessage {
        id: id.into(),
        role: role.into(),
        text: text.into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }
}

#[test]
fn extract_post_tool_tail_before_tool_takes_loading_with_text() {
    let mut msgs = vec![StoredMessage {
        id: "a_load".into(),
        role: "assistant".into(),
        text: "完成。".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    let peeled = extract_post_tool_tail_before_tool(&mut msgs, "a_load").expect("extracted");
    assert_eq!(peeled.text, "完成。");
    assert!(msgs.is_empty());
}

#[test]
fn extract_post_tool_tail_skips_empty_loading() {
    let mut msgs = vec![StoredMessage {
        id: "a_load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    assert!(extract_post_tool_tail_before_tool(&mut msgs, "a_load").is_none());
    assert_eq!(msgs.len(), 1);
}

#[test]
fn extract_post_tool_tail_prefers_premature_finalized_row() {
    let mut msgs = vec![StoredMessage {
        id: "a_done".into(),
        role: "assistant".into(),
        text: "已定稿。".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    let peeled = extract_post_tool_tail_before_tool(&mut msgs, "a_done").expect("extracted");
    assert_eq!(peeled.text, "已定稿。");
    assert!(msgs.is_empty());
}

#[test]
fn post_tool_tool_boundary_creates_empty_loading_tail() {
    let mut msgs = vec![
        empty_msg("t0", "system", "tool", true),
        StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: "完成。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    let _peeled = extract_post_tool_tail_before_tool(&mut msgs, "a_load").expect("peeled");
    insert_tool_row(
        &mut msgs,
        empty_msg("t1", "system", "next tool", true),
        None,
    );
    msgs.insert(
        2,
        StoredMessage {
            id: "a_new".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    );
    pin_loading_tail_in_messages(&mut msgs, "a_new");
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[2].id, "a_new");
    assert!(
        msgs[2].text.is_empty(),
        "P0: canonical sync fills tail, not peel merge"
    );
}

#[test]
fn sync_loading_tail_block_writes_streaming_tail() {
    let mut msgs = vec![StoredMessage {
        id: "a_load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    sync_loading_tail_block_in_messages(&mut msgs, "a_load", "完成。");
    assert_eq!(msgs[0].text, "完成。");
}

#[test]
fn finalize_loading_when_tail_matches_respects_post_tool_flag() {
    assert!(TurnLayout::should_finalize_loading_when_tail_matches_final_response(false));
    assert!(!TurnLayout::should_finalize_loading_when_tail_matches_final_response(true));
}

#[test]
fn peel_removes_finalized_post_tool_tail_only() {
    let mut msgs = vec![
        empty_msg("t0", "system", "tool", true),
        StoredMessage {
            id: "a_done".into(),
            role: "assistant".into(),
            text: "完成。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    let peeled = peel_premature_summary_from_messages(&mut msgs, "a_done").expect("peeled");
    assert_eq!(
        peeled,
        PeeledSummary {
            text: "完成。".into(),
            reasoning_text: String::new(),
        }
    );
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].id, "t0");
}

#[test]
fn peel_skips_loading_tail() {
    let mut msgs = vec![StoredMessage {
        id: "a_load".into(),
        role: "assistant".into(),
        text: "续写中".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    assert!(peel_premature_summary_from_messages(&mut msgs, "a_load").is_none());
    assert_eq!(msgs.len(), 1);
}

#[test]
fn finalize_loading_row_at_removes_empty_shell() {
    let mut msgs = vec![StoredMessage {
        id: "a_load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    finalize_loading_row_at(&mut msgs, 0);
    assert!(msgs.is_empty());
}

#[test]
fn pin_loading_tail_in_messages_moves_loading_to_end() {
    let mut msgs = vec![
        empty_msg("t0", "system", "tool", true),
        StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: "续写".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    pin_loading_tail_in_messages(&mut msgs, "a_load");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].id, "a_load");
}

#[test]
fn late_tool_order_tool_then_empty_loading_tail() {
    let mut msgs = vec![
        empty_msg("t0", "system", "create file", true),
        StoredMessage {
            id: "a_done".into(),
            role: "assistant".into(),
            text: "完成。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    let _peeled = peel_premature_summary_from_messages(&mut msgs, "a_done").expect("peeled");
    msgs.push(empty_msg("t1", "system", "cmake", true));
    msgs.push(StoredMessage {
        id: "a_load".into(),
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
    assert_eq!(msgs[0].id, "t0");
    assert_eq!(msgs[1].id, "t1");
    assert_eq!(msgs[2].id, "a_load");
    assert!(msgs[2].text.is_empty());
}
