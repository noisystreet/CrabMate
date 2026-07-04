use super::*;
use crate::storage::{StoredMessage, StoredMessageState};
use crabmate_turn_layout::Turn;

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
fn relocate_removes_commentary_stray_after_tool_and_keeps_anchor_before_tool() {
    let commentary = "好的，我来看看当前工作目录的情况。";
    let mut msgs = vec![
        empty_msg("intent", "assistant", "意图分析", false),
        StoredMessage {
            id: "anchored".into(),
            role: "assistant".into(),
            text: "旧旁注".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: Some("tc_list".into()),
            tool_name: None,
            created_at: 0,
        },
        StoredMessage {
            id: "tc_list".into(),
            role: "system".into(),
            text: "list_tree".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_list".into()),
            tool_name: Some("list_tree".into()),
            created_at: 0,
        },
        StoredMessage {
            id: "a_final".into(),
            role: "assistant".into(),
            text: "当前工作目录下有三个压缩包。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
        StoredMessage {
            id: "stray".into(),
            role: "assistant".into(),
            text: commentary.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    let turn = {
        let mut t = Turn::default();
        crabmate_turn_layout::reduce_event(
            &mut t,
            crabmate_turn_layout::TurnEvent::ToolCall {
                tool_call_id: "tc_list".into(),
                name: "list_tree".into(),
                summary: "list tree".into(),
            },
        );
        t.steps[0].before_commentary = Some(commentary.to_string());
        t
    };
    relocate_misplaced_commentary_rows(&mut msgs, &turn);
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[1].id, "anchored");
    assert_eq!(msgs[1].text, commentary);
    assert_eq!(msgs[3].id, "a_final");
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
fn remove_redundant_loading_tail_at_drops_duplicate_shell() {
    let final_text = "HPCG 编译完成 ✅";
    let mut msgs = vec![
        empty_msg("t0", "system", "tool", true),
        StoredMessage {
            id: "a_final".into(),
            role: "assistant".into(),
            text: final_text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
        StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: final_text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    assert!(remove_redundant_loading_tail_at(&mut msgs, 2, final_text));
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].id, "a_final");
    assert_eq!(msgs[1].text, final_text);
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
