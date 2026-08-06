use super::*;
use crate::storage::StoredMessageState;
use crate::stream_text_overlay::StreamTextOverlay;
use std::collections::HashMap;

fn sync(
    prev: Option<&TuiMountState>,
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    tool_chunks: &HashMap<String, String>,
) -> TuiSyncPlan {
    plan_tui_sync(PlanTuiSyncArgs {
        prev,
        messages,
        session_id,
        overlay,
        locale: Locale::ZhHans,
        apply_assistant_display_filters: false,
        markdown_render: true,
        tool_chunks,
    })
}

fn message(id: &str, role: &str, text: &str) -> StoredMessage {
    StoredMessage {
        id: id.to_string(),
        role: role.to_string(),
        text: text.to_string(),
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
fn streaming_tokens_incremental_open_plain() {
    let user = message("u1", "user", "hi");
    let mut assistant = message("a1", "assistant", "");
    assistant.state = Some(StoredMessageState::Loading);
    let messages = vec![user, assistant];
    let overlay1 = StreamTextOverlay {
        session_id: "s1".to_string(),
        message_id: "a1".to_string(),
        answer: "**a".to_string(),
        reasoning: String::new(),
    };
    let empty = HashMap::new();
    let plan1 = sync(None, &messages, "s1", Some(&overlay1), &empty);
    assert!(plan1.full_html.is_some());

    let overlay2 = StreamTextOverlay {
        session_id: "s1".to_string(),
        message_id: "a1".to_string(),
        answer: "**ab".to_string(),
        reasoning: String::new(),
    };
    let plan2 = sync(Some(&plan1.next), &messages, "s1", Some(&overlay2), &empty);
    assert!(plan2.full_html.is_none());
    assert!(plan2.append_sections.is_empty());
    let live = plan2.live.expect("live patch");
    assert_eq!(live.message_id, "a1");
    match live.patch {
        TuiBodyPatch::Incremental {
            append_closed,
            open_plain,
        } => {
            assert!(append_closed.is_empty());
            assert_eq!(open_plain.as_deref(), Some("**ab"));
        }
        other => panic!("expected Incremental, got {other:?}"),
    }
}

#[test]
fn empty_loading_shell_not_appended_until_overlay_has_text() {
    let user = message("u1", "user", "hi");
    let empty = HashMap::new();
    let plan1 = sync(None, std::slice::from_ref(&user), "s1", None, &empty);
    assert!(plan1.full_html.is_some());
    assert_eq!(plan1.next.mounted_ids, vec!["u1".to_string()]);

    let mut assistant = message("a1", "assistant", "");
    assistant.state = Some(StoredMessageState::Loading);
    let messages = vec![user.clone(), assistant];
    let empty_overlay = StreamTextOverlay {
        session_id: "s1".to_string(),
        message_id: "a1".to_string(),
        answer: String::new(),
        reasoning: String::new(),
    };
    let plan2 = sync(
        Some(&plan1.next),
        &messages,
        "s1",
        Some(&empty_overlay),
        &empty,
    );
    assert!(plan2.full_html.is_none());
    assert!(
        plan2.append_sections.is_empty(),
        "empty loading shell must not mount"
    );
    assert_eq!(plan2.next.mounted_ids, vec!["u1".to_string()]);

    let with_text = StreamTextOverlay {
        session_id: "s1".to_string(),
        message_id: "a1".to_string(),
        answer: "你好".to_string(),
        reasoning: String::new(),
    };
    let plan3 = sync(Some(&plan2.next), &messages, "s1", Some(&with_text), &empty);
    assert!(plan3.full_html.is_none());
    assert_eq!(plan3.append_sections.len(), 1);
    assert!(plan3.append_sections[0].contains("data-tui-live=\"1\""));
    assert!(plan3.append_sections[0].contains("你好"));
    assert!(
        plan3.live.is_none(),
        "new live body already in section html"
    );
}

#[test]
fn session_switch_forces_full_rebuild() {
    let messages = vec![message("u1", "user", "hi")];
    let empty = HashMap::new();
    let plan1 = sync(None, &messages, "s1", None, &empty);
    let plan2 = sync(Some(&plan1.next), &messages, "s2", None, &empty);
    assert!(plan2.full_html.is_some());
}

#[test]
fn finished_assistant_bold_becomes_strong() {
    let messages = vec![
        message("u1", "user", "你好"),
        message("a1", "assistant", "**原样**"),
    ];
    let output = build_tui_transcript_html(
        &messages,
        "s1",
        None,
        Locale::ZhHans,
        false,
        true,
        &HashMap::new(),
    );
    assert!(output.contains("data-tui-msg-id"), "got {output}");
    assert!(output.contains("chat-tui-turn--user"), "got {output}");
    assert!(output.contains("chat-tui-turn--assistant"), "got {output}");
    assert!(output.contains("chat-tui-role-label"), "got {output}");
    for section in output.split("<section class=\"chat-tui-turn") {
        if let Some(end) = section.find("</section>") {
            assert!(
                !section[..end].contains("chat-tui-role-label"),
                "role label must be outside section card, got {output}"
            );
        }
    }
    assert!(!output.contains("chat-tui-turn-actions"), "got {output}");
    assert!(!output.contains("data-tui-action=\"copy\""), "got {output}");
    assert!(output.contains("data-tui-msg-idx"), "got {output}");
    assert!(!output.contains('❯'), "got {output}");
    assert!(
        output.contains("<strong>") || output.contains("<b>"),
        "got {output}"
    );
}

#[test]
fn tool_turn_uses_tool_modifier_without_generic_role_word() {
    let mut tool = message("t1", "assistant", "ok");
    tool.is_tool = true;
    tool.tool_name = Some("read_file".to_string());
    let output = build_tui_transcript_html(
        &[tool],
        "s1",
        None,
        Locale::ZhHans,
        false,
        true,
        &HashMap::new(),
    );
    assert!(output.contains("chat-tui-turn--tool"), "got {output}");
    assert!(output.contains("chat-tui-tool-process"), "got {output}");
    assert!(output.contains("read_file"), "got {output}");
    assert!(!output.contains("工具:"), "got {output}");
}

#[test]
fn promote_finished_assistant_uses_incremental_not_replace_all() {
    let user = message("u1", "user", "hi");
    let mut assistant = message("a1", "assistant", "");
    assistant.state = Some(StoredMessageState::Loading);
    let messages_loading = vec![user.clone(), assistant.clone()];
    let overlay = StreamTextOverlay {
        session_id: "s1".to_string(),
        message_id: "a1".to_string(),
        answer: "hello\n\n**tail**".to_string(),
        reasoning: String::new(),
    };
    let empty = HashMap::new();
    let plan1 = sync(None, &messages_loading, "s1", Some(&overlay), &empty);
    assert!(plan1.next.live_id.as_deref() == Some("a1"));

    assistant.state = None;
    assistant.text = "hello\n\n**tail**".to_string();
    let messages_done = vec![user, assistant];
    let plan2 = sync(Some(&plan1.next), &messages_done, "s1", None, &empty);
    assert!(plan2.next.live_id.is_none());
    let live = plan2.live.expect("promote patch");
    assert_eq!(live.message_id, "a1");
    match live.patch {
        TuiBodyPatch::Incremental { .. } => {}
        TuiBodyPatch::ReplaceAll { .. } => {
            panic!("promote must not ReplaceAll whole body (jitter)")
        }
        other => panic!("unexpected patch {other:?}"),
    }
}

#[test]
fn live_tool_chunk_uses_tool_row_patch_not_replace_all() {
    let user = message("u1", "user", "hi");
    let mut tool = message("t1", "assistant", "");
    tool.is_tool = true;
    tool.tool_name = Some("read_file".to_string());
    tool.tool_call_id = Some("tc1".to_string());
    tool.state = Some(StoredMessageState::Loading);
    let messages = vec![user, tool];
    let mut chunks = HashMap::new();
    chunks.insert("tc1".to_string(), "part-a".to_string());
    let plan1 = sync(None, &messages, "s1", None, &chunks);
    assert!(plan1.full_html.is_some());
    assert_eq!(plan1.next.live_tool_has_details, Some(false));

    chunks.insert("tc1".to_string(), "part-a part-b".to_string());
    let plan2 = sync(Some(&plan1.next), &messages, "s1", None, &chunks);
    assert!(plan2.full_html.is_none());
    let live = plan2.live.expect("tool live patch");
    assert_eq!(live.message_id, "t1");
    match live.patch {
        TuiBodyPatch::ToolRow {
            status,
            status_label,
            one_line,
            detail,
        } => {
            assert_eq!(status, "⏳");
            assert!(
                status_label.contains("执行") || status_label.contains("Running"),
                "{status_label}"
            );
            assert!(one_line.contains("part-b"), "{one_line}");
            assert!(detail.is_none());
        }
        other => panic!("expected ToolRow, got {other:?}"),
    }
}

#[test]
fn skill_slash_chip_stays_on_same_line_as_task() {
    let user = message("u1", "user", "/rust-style 分析一下");
    let empty = HashMap::new();
    let html = build_tui_transcript_html(
        std::slice::from_ref(&user),
        "s1",
        None,
        Locale::ZhHans,
        false,
        true,
        &empty,
    );
    assert!(html.contains("msg-skill-invoke"), "{html}");
    assert!(html.contains("rust-style"), "{html}");
    assert!(html.contains("分析一下"), "{html}");
    // 旧实现：裸 chip 后再跟独立 chat-tui-line，视觉上多一换行。
    assert!(
        !html.contains("</span> <div class=\"chat-tui-line"),
        "chip must not sit outside the first line block: {html}"
    );
    assert!(
        html.contains("msg-skill-invoke") && html.contains("chat-tui-line"),
        "{html}"
    );
    let chip_at = html.find("msg-skill-invoke").expect("chip");
    let line_at = html.find("chat-tui-line").expect("line");
    assert!(
        line_at < chip_at,
        "chip should be inside a line wrapper: {html}"
    );
}

#[test]
fn file_ref_chip_stays_on_same_line_as_following_text() {
    let user = message("u1", "user", "@.gitignore 这个文件是什么");
    let empty = HashMap::new();
    let html = build_tui_transcript_html(
        std::slice::from_ref(&user),
        "s1",
        None,
        Locale::ZhHans,
        false,
        true,
        &empty,
    );
    assert!(html.contains("msg-file-ref"), "{html}");
    assert!(html.contains(".gitignore"), "{html}");
    assert!(html.contains("这个文件是什么"), "{html}");
    assert!(
        !html.contains("</span><div class=\"chat-tui-line")
            && !html.contains("</span> <div class=\"chat-tui-line"),
        "file-ref chip must not sit outside the line block: {html}"
    );
    let chip_at = html.find("msg-file-ref").expect("chip");
    let line_at = html.find("chat-tui-line").expect("line");
    assert!(
        line_at < chip_at,
        "file-ref chip should be inside a line wrapper: {html}"
    );
    // 占位符不得泄漏到最终 HTML。
    assert!(!html.contains("CMFR"), "{html}");
}
