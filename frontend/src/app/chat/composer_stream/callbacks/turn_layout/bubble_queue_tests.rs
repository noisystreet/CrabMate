use super::*;
use crate::sse_dispatch::TurnSegmentStartInfo;

fn make_turn_with_commentary() -> TurnCanonicalState {
    let mut turn = TurnCanonicalState::new();
    turn.on_segment_start(TurnSegmentStartInfo {
        segment_id: "seg-before-tc_a".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_a".into()),
    });
    assert!(turn.try_apply_commentary_delta("步骤 A。"));
    turn.on_segment_end("seg-before-tc_a".into());
    turn.on_tool_call("tc_a", "tool_a", "tool a");
    turn
}

#[test]
fn loading_preview_during_tool_phase_skips_anchored_open_commentary() {
    let mut turn = TurnCanonicalState::new();
    turn.on_segment_start(TurnSegmentStartInfo {
        segment_id: "seg-before-tc_a".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_a".into()),
    });
    assert!(turn.try_apply_commentary_delta("步骤 A。"));
    turn.on_segment_end("seg-before-tc_a".into());
    turn.on_tool_call("tc_a", "tool_a", "tool a");
    turn.on_segment_start(TurnSegmentStartInfo {
        segment_id: "seg-before-tc_b".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_b".into()),
    });
    assert!(turn.try_apply_commentary_delta("步骤 B。"));
    assert_eq!(
        crabmate_turn_layout::commentary_for_tool(turn.turn_ref(), "tc_a").as_deref(),
        Some("步骤 A。")
    );
    assert!(
        BubbleOutputQueue::loading_preview_text(&turn, None, None).is_empty(),
        "Phase B: anchored open commentary must not paint on loading overlay"
    );
}

#[test]
fn upsert_parks_anchored_commentary_before_loading_until_tool_exists() {
    let mut msgs = vec![
        crate::storage::StoredMessage {
            id: "u".into(),
            role: "user".into(),
            text: "q".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    assert!(BubbleOutputQueue::upsert_streaming_anchored_commentary(
        &mut msgs,
        "tc_list",
        "先看目录。".into(),
        Some("load"),
    ));
    assert_eq!(msgs[1].id, commentary_row_id("tc_list"));
    assert_eq!(msgs[1].text, "先看目录。");
    assert_eq!(msgs[2].id, "load");

    msgs.insert(
        2,
        crate::storage::StoredMessage {
            id: "t".into(),
            role: "system".into(),
            text: "list".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_list".into()),
            tool_name: None,
            created_at: 0,
        },
    );
    // commentary still before tool
    assert_eq!(msgs[1].id, commentary_row_id("tc_list"));
    assert_eq!(msgs[2].id, "t");
    assert!(BubbleOutputQueue::upsert_streaming_anchored_commentary(
        &mut msgs,
        "tc_list",
        "先看目录。继续。".into(),
        Some("load"),
    ));
    assert_eq!(msgs[1].text, "先看目录。继续。");
    assert_eq!(msgs[2].id, "t");
}

#[test]
fn flush_commentary_inserts_immutable_row_before_its_tool() {
    let turn = make_turn_with_commentary();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![crate::storage::StoredMessage {
        id: "t".into(),
        role: "system".into(),
        text: "tool a".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: true,
        tool_call_id: Some("tc_a".into()),
        tool_name: None,
        created_at: 0,
    }];
    queue.flush_commentary_rows(&mut msgs, &turn, None);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].id, commentary_row_id("tc_a"));
    assert_eq!(msgs[0].text, "步骤 A。");
    assert_eq!(msgs[1].id, "t");
    queue.flush_commentary_rows(&mut msgs, &turn, None);
    assert_eq!(msgs.len(), 2, "second flush must not duplicate row");
}

/// 晚到旁注：工具行已在列表中时，流式 upsert 须插在工具之前（勿落 loading 尾）。
#[test]
fn upsert_late_streaming_commentary_before_existing_tool() {
    let mut msgs = vec![
        crate::storage::StoredMessage {
            id: "t".into(),
            role: "system".into(),
            text: "create".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_create".into()),
            tool_name: None,
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    assert!(BubbleOutputQueue::upsert_commentary_before_tool(
        &mut msgs,
        "tc_create",
        "工作区是空的。".into(),
    ));
    assert_eq!(msgs[0].id, commentary_row_id("tc_create"));
    assert_eq!(msgs[0].text, "工作区是空的。");
    assert_eq!(msgs[1].id, "t");
    assert!(BubbleOutputQueue::upsert_commentary_before_tool(
        &mut msgs,
        "tc_create",
        "工作区是空的。继续。".into(),
    ));
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].text, "工作区是空的。继续。");
    assert_eq!(msgs[1].id, "t");
}

#[test]
fn loading_preview_empty_for_anchored_open_commentary_even_without_tool_row() {
    let mut turn = TurnCanonicalState::new();
    turn.on_segment_start(TurnSegmentStartInfo {
        segment_id: "seg-late".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_create".into()),
    });
    assert!(turn.try_apply_commentary_delta("晚到旁白。"));
    assert!(
        BubbleOutputQueue::loading_preview_text(&turn, None, None).is_empty(),
        "anchored open commentary never uses loading overlay (Phase B)"
    );
    let msgs = vec![crate::storage::StoredMessage {
        id: "t".into(),
        role: "system".into(),
        text: "tool".into(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: true,
        tool_call_id: Some("tc_create".into()),
        tool_name: None,
        created_at: 0,
    }];
    assert!(BubbleOutputQueue::loading_preview_text(&turn, None, Some(&msgs)).is_empty());
}

#[test]
fn sync_web_projection_keeps_loading_body() {
    let mut turn = TurnCanonicalState::new();
    assert!(turn.try_apply_answer_state_transition("完成。"));
    turn.on_tool_phase_end();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![
        crate::storage::StoredMessage {
            id: commentary_row_id("tc_existing"),
            role: "assistant".into(),
            text: "说明。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: "不应落盘的尾泡正文".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    // 终答在 overlay；模拟 overlay 已有终答。
    queue.sync_web_projection(&mut msgs, &turn, Some("load"), Some("完成。"), true);
    // loading tail 保留正文（不再清空，避免聊天列气泡闪烁）
    let load = msgs.iter().find(|m| m.id == "load").expect("loading shell");
    assert_eq!(load.text, "不应落盘的尾泡正文");
    assert!(
        msgs.iter()
            .any(|m| m.id == FINAL_ANSWER_ROW_ID && m.text == "完成。")
    );
}

#[test]
fn flush_commentary_moves_misordered_row_before_tool() {
    let mut turn = TurnCanonicalState::new();
    turn.on_tool_call("tc_archive", "archive_list", "list");
    turn.on_segment_start(crate::sse_dispatch::TurnSegmentStartInfo {
        segment_id: "seg-before-tc_unpack".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_unpack".into()),
    });
    assert!(turn.try_apply_commentary_delta("好的，先解压。"));
    turn.on_tool_call("tc_unpack", "unpack", "unpack");

    let queue = BubbleOutputQueue;
    let mut msgs = vec![
        crate::storage::StoredMessage {
            id: "tc_archive".into(),
            role: "system".into(),
            text: "archive".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_archive".into()),
            tool_name: None,
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: "tc_unpack".into(),
            role: "system".into(),
            text: "unpack".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_unpack".into()),
            tool_name: None,
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: commentary_row_id("tc_unpack"),
            role: "assistant".into(),
            text: "好的，先解压。".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    queue.flush_commentary_rows(&mut msgs, &turn, None);
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].id, "tc_archive");
    assert_eq!(msgs[1].id, commentary_row_id("tc_unpack"));
    assert_eq!(msgs[2].id, "tc_unpack");
}

#[test]
fn flush_final_deferred_until_commentary_row_present() {
    let mut turn = TurnCanonicalState::new();
    turn.on_tool_call("tc_a", "tool_a", "tool a");
    turn.on_segment_start(crate::sse_dispatch::TurnSegmentStartInfo {
        segment_id: "seg-before-tc_a".into(),
        kind: "commentary".into(),
        before_tool_call_id: Some("tc_a".into()),
    });
    assert!(turn.try_apply_commentary_delta("批说明。"));
    turn.on_segment_end("seg-before-tc_a".into());
    turn.on_tool_phase_end();
    assert!(turn.try_apply_answer_state_transition("终答。"));

    let queue = BubbleOutputQueue;
    let mut msgs = vec![crate::storage::StoredMessage {
        id: "load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(crate::storage::StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    // 终答在 overlay；模拟 overlay 已有终答。
    queue.flush_final_answer_row(&mut msgs, &turn, Some("load"), Some("终答。"));
    assert!(
        !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
        "final must not appear before commentary row"
    );

    msgs.insert(
        0,
        crate::storage::StoredMessage {
            id: "tc_a".into(),
            role: "system".into(),
            text: "tool".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_a".into()),
            tool_name: None,
            created_at: 0,
        },
    );
    queue.sync_web_projection(&mut msgs, &turn, Some("load"), Some("终答。"), true);
    let commentary_idx = msgs
        .iter()
        .position(|m| m.id == commentary_row_id("tc_a"))
        .expect("commentary");
    let final_idx = msgs
        .iter()
        .position(|m| m.id == FINAL_ANSWER_ROW_ID)
        .expect("final");
    assert!(
        commentary_idx < final_idx,
        "commentary must precede final in stored order"
    );
}

#[test]
fn flush_commentary_skips_without_tool_row() {
    let turn = make_turn_with_commentary();
    let queue = BubbleOutputQueue;
    let mut msgs = Vec::new();
    queue.flush_commentary_rows(&mut msgs, &turn, None);
    assert!(msgs.is_empty());
}

/// 工具前旁白仍在 overlay：`allow_final_answer=false` 时不得写入 turn-final-answer。
#[test]
fn pre_tool_sync_skips_final_answer_when_not_allowed() {
    let turn = TurnCanonicalState::new();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![crate::storage::StoredMessage {
        id: "load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(crate::storage::StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    queue.sync_web_projection(
        &mut msgs,
        &turn,
        Some("load"),
        Some("好的，这是一个 C++ 项目。"),
        false,
    );
    assert!(
        !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
        "pre-tool commentary must not become turn-final-answer"
    );
    assert!(msgs.iter().any(|m| m.id == "load"));
}

/// 多步工具之间：即使会话已有工具行，中间旁白 overlay 在 `allow=false` 时不得进终答。
#[test]
fn mid_process_overlay_skips_final_answer_when_not_allowed() {
    let turn = TurnCanonicalState::new();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![
        crate::storage::StoredMessage {
            id: "tc_list".into(),
            role: "system".into(),
            text: "list".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: Some("tc_list".into()),
            tool_name: Some("list_tree".into()),
            created_at: 0,
        },
        crate::storage::StoredMessage {
            id: "load".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(crate::storage::StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        },
    ];
    queue.sync_web_projection(
        &mut msgs,
        &turn,
        Some("load"),
        Some("工作区是空的。我来创建程序。"),
        false,
    );
    assert!(
        !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
        "mid-process narration must not become turn-final-answer"
    );
}

/// 无工具场景：`flush_final_answer_row` 从 overlay 创建 FINAL_ANSWER_ROW。
///
/// 这是无工具问答的正常路径：流式 delta 写入 overlay，on_done 时
/// `flush_final_answer_row` 读 overlay 创建终答行。
#[test]
fn no_tool_flush_final_creates_row_from_overlay() {
    let turn = TurnCanonicalState::new();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![crate::storage::StoredMessage {
        id: "load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(crate::storage::StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    queue.sync_web_projection(
        &mut msgs,
        &turn,
        Some("load"),
        Some("无工具终答正文。"),
        true,
    );
    // FINAL_ANSWER_ROW 应创建
    let final_row = msgs
        .iter()
        .find(|m| m.id == FINAL_ANSWER_ROW_ID)
        .expect("FINAL_ANSWER_ROW must be created in no-tool scenario");
    assert_eq!(final_row.text, "无工具终答正文。");
    // loading tail 仍保留
    assert!(msgs.iter().any(|m| m.id == "load"));
}

/// 无工具场景 overlay 为空时：`flush_final_answer_row` 不应创建
/// FINAL_ANSWER_ROW（对应 overlay 被 prematurely 清空的情况）。
#[test]
fn no_tool_flush_final_skips_when_overlay_empty() {
    let turn = TurnCanonicalState::new();
    let queue = BubbleOutputQueue;
    let mut msgs = vec![crate::storage::StoredMessage {
        id: "load".into(),
        role: "assistant".into(),
        text: String::new(),
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(crate::storage::StoredMessageState::Loading),
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    }];
    queue.sync_web_projection(&mut msgs, &turn, Some("load"), None, true);
    // overlay 为空时不应创建 FINAL_ANSWER_ROW
    assert!(
        !msgs.iter().any(|m| m.id == FINAL_ANSWER_ROW_ID),
        "FINAL_ANSWER_ROW must not be created when overlay is empty"
    );
}
