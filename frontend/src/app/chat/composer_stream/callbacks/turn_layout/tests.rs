use super::projection_reconciler::{insert_tool_row, pin_loading_tail_in_messages};
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
fn finalize_loading_row_at_removes_row_with_text() {
    let mut msgs = vec![
        empty_msg("u1", "user", "q", false),
        StoredMessage {
            id: "a_load".into(),
            role: "assistant".into(),
            text: "流式正文预览".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    finalize_loading_row_at(&mut msgs, 1);
    assert_eq!(
        msgs.len(),
        2,
        "loading row with text must be kept (finalized, not removed; on_done dedup handles duplicate projection rows)"
    );
    assert_eq!(msgs[1].id, "a_load");
    assert!(
        msgs[1].state.is_none(),
        "loading state must be cleared after finalize"
    );
    assert_eq!(msgs[1].text, "流式正文预览", "text must be preserved");
}

#[test]
fn finalize_loading_drops_text_already_on_commentary_row() {
    let mut msgs = vec![
        StoredMessage {
            id: "turn-commentary-tc1".into(),
            role: "assistant".into(),
            text: "旁白正文。".into(),
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
            text: "旁白正文。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    finalize_loading_row_at(&mut msgs, 1);
    assert_eq!(
        msgs.len(),
        1,
        "demoted loading must not become a second ready row"
    );
    assert_eq!(msgs[0].id, "turn-commentary-tc1");
    assert_eq!(msgs[0].text, "旁白正文。");
}

#[test]
fn finalize_loading_drops_text_already_on_final_answer_row() {
    let mut msgs = vec![
        StoredMessage {
            id: "turn-final-answer".into(),
            role: "assistant".into(),
            text: "中间旁白。".into(),
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
            text: "中间旁白。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    finalize_loading_row_at(&mut msgs, 1);
    assert_eq!(
        msgs.len(),
        1,
        "loading must not duplicate turn-final-answer mid-process"
    );
    assert_eq!(msgs[0].id, "turn-final-answer");
}

#[test]
fn allow_final_answer_flush_requires_gate_or_stream_end() {
    assert!(!TurnLayout::should_allow_final_answer_flush(false, false));
    assert!(TurnLayout::should_allow_final_answer_flush(true, false));
    assert!(TurnLayout::should_allow_final_answer_flush(false, true));
    assert!(TurnLayout::should_allow_final_answer_flush(true, true));
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

#[test]
fn should_clear_overlay_during_tool_phase_when_commentary_owns_text() {
    use crate::app::chat::composer_stream::turn_canonical::TurnCanonicalState;
    use crate::sse_dispatch::TurnSegmentStartInfo;

    let mut turn = TurnCanonicalState::new();
    turn.on_segment_start(TurnSegmentStartInfo {
        segment_id: "seg-c".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc1".into()),
    });
    assert!(turn.try_apply_commentary_delta("旁白正文。"));
    turn.on_segment_end("seg-c".into());
    turn.on_tool_call("tc1", "read_file", "read");
    assert!(turn.tool_phase_open());
    let msgs = vec![StoredMessage {
        id: "turn-commentary-tc1".into(),
        role: "assistant".into(),
        text: "旁白正文。".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    assert!(
        should_clear_preview_overlay_answer(&turn, &msgs, Some("旁白正文。")),
        "I14: tool phase clears overlay once commentary owns the same text"
    );
    assert!(
        !should_clear_preview_overlay_answer(&turn, &msgs, Some("尚未投影的旁白。")),
        "must keep overlay when commentary does not own this live text"
    );
}
