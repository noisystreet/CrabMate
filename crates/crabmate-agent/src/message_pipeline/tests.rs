use crabmate_types::{FunctionCall, Message, ToolCall};

use super::{
    MessagePipelineConfig, MessagePipelineReport, MessagePipelineStage,
    apply_session_sync_pipeline_with_config, compress_tool_message_contents,
    conversation_messages_to_vendor_body, drop_orphan_tool_messages, trim_messages_by_char_budget,
    trim_messages_by_count,
};

fn tool_msg(s: &str) -> Message {
    Message {
        role: "tool".to_string(),
        content: Some(s.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: Some("1".into()),
    }
}

#[test]
fn compress_tool_truncates() {
    let long = "x".repeat(2000);
    let mut v = vec![tool_msg(&long)];
    compress_tool_message_contents(&mut v, 256);
    let c = crabmate_types::message_content_as_str(&v[0].content).unwrap();
    assert!(c.starts_with(&"x".repeat(256)));
    assert!(c.contains("截断"));
    assert!(c.chars().count() < long.chars().count());
}

#[test]
fn trim_by_count_keeps_system_and_tail() {
    let mut v = vec![
        Message {
            role: "system".to_string(),
            content: Some("s".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some("a".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "assistant".to_string(),
            content: Some("b".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some("c".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    trim_messages_by_count(&mut v, 2);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].role, "system");
    assert_eq!(
        crabmate_types::message_content_as_str(&v[1].content),
        Some("b")
    );
    assert_eq!(
        crabmate_types::message_content_as_str(&v[2].content),
        Some("c")
    );
}

#[test]
fn trim_by_count_inserts_user_when_tail_would_be_two_assistants() {
    let mut v = vec![
        Message::system_only("s"),
        Message::user_only("old_u"),
        Message {
            role: "assistant".to_string(),
            content: Some("a1".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "assistant".to_string(),
            content: Some("a2".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    trim_messages_by_count(&mut v, 2);
    assert_eq!(v.len(), 3);
    assert_eq!(v[1].role, "user");
    assert_eq!(
        crabmate_types::message_content_as_str(&v[1].content),
        Some("old_u")
    );
    assert_eq!(v[2].role, "assistant");
}

#[test]
fn char_budget_drops_oldest_after_system() {
    let mut v = vec![
        Message {
            role: "system".to_string(),
            content: Some("s".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some("aaaaaaaaaa".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some("bbbbbbbbbbbbbbbb".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    trim_messages_by_char_budget(&mut v, 6, 1);
    assert_eq!(v.len(), 2);
    assert_eq!(
        crabmate_types::message_content_as_str(&v[1].content),
        Some("bbbbbbbbbbbbbbbb")
    );
}

fn assistant_with_tool_calls() -> Message {
    Message {
        role: "assistant".to_string(),
        content: None,
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "x".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    }
}

#[test]
fn drop_orphan_tool_removes_leading_tools_after_trim() {
    let mut v = vec![
        Message {
            role: "system".to_string(),
            content: Some("s".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        tool_msg("orphan1"),
        tool_msg("orphan2"),
        Message {
            role: "user".to_string(),
            content: Some("last".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    drop_orphan_tool_messages(&mut v);
    assert_eq!(v.len(), 2);
    assert_eq!(v[1].role, "user");
    assert_eq!(
        crabmate_types::message_content_as_str(&v[1].content),
        Some("last")
    );
}

#[test]
fn drop_orphan_tool_keeps_chain_after_assistant_tool_calls() {
    let mut v = vec![
        Message {
            role: "system".to_string(),
            content: Some("s".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        assistant_with_tool_calls(),
        tool_msg("a"),
        tool_msg("b"),
        Message {
            role: "user".to_string(),
            content: Some("u".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    drop_orphan_tool_messages(&mut v);
    assert_eq!(v.len(), 5);
}

#[test]
fn drop_orphan_tool_removes_tool_after_assistant_without_tool_calls() {
    let mut v = vec![
        Message {
            role: "system".to_string(),
            content: Some("s".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "assistant".to_string(),
            content: Some("text only".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        tool_msg("bad"),
    ];
    drop_orphan_tool_messages(&mut v);
    assert_eq!(v.len(), 2);
}

#[test]
fn vendor_body_matches_manual_strip_normalize() {
    let sep = Message::chat_ui_separator(true);
    let a = Message {
        role: "assistant".to_string(),
        content: Some("c".into()),
        reasoning_content: Some("r".to_string()),
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let slice = [Message::user_only("u"), sep, a.clone()];
    let via = conversation_messages_to_vendor_body(&slice, false, false, false);
    let manual = crabmate_types::normalize_messages_for_openai_compatible_request(
        crabmate_types::messages_for_api_stripping_reasoning_skip_ui_separators(
            &slice, false, false,
        ),
    );
    assert_eq!(via, manual);
}

#[test]
fn pipeline_report_skips_char_budget_stages_when_budget_zero() {
    let mut v = vec![
        Message::system_only("s"),
        Message::user_only("a"),
        Message::user_only("b"),
    ];
    let cfg = MessagePipelineConfig {
        tool_message_max_chars: 512,
        max_message_history: 10,
        context_char_budget: 0,
        context_min_messages_after_system: 1,
    };
    let mut report = MessagePipelineReport::default();
    apply_session_sync_pipeline_with_config(&mut v, cfg, Some(&mut report));
    let stages: Vec<MessagePipelineStage> = report.steps.iter().map(|s| s.stage).collect();
    assert!(
        !stages.contains(&MessagePipelineStage::AfterTrimByCharBudget),
        "budget=0 不应出现 AfterTrimByCharBudget: {:?}",
        stages
    );
    assert!(
        !stages.contains(&MessagePipelineStage::AfterSecondCompressTool),
        "budget=0 不应出现 AfterSecondCompressTool: {:?}",
        stages
    );
    assert_eq!(
        stages.first(),
        Some(&MessagePipelineStage::SessionSyncStart)
    );
    assert_eq!(
        stages.last(),
        Some(&MessagePipelineStage::AfterMergeAssistantsInPlace)
    );
}

#[test]
fn pipeline_report_includes_char_trim_and_second_compress_when_budget_positive() {
    let mut v = vec![
        Message::system_only("s"),
        Message::user_only("x".repeat(100)),
        Message::user_only("y".repeat(100)),
    ];
    let cfg = MessagePipelineConfig {
        tool_message_max_chars: 512,
        max_message_history: 10,
        context_char_budget: 50,
        context_min_messages_after_system: 1,
    };
    let mut report = MessagePipelineReport::default();
    apply_session_sync_pipeline_with_config(&mut v, cfg, Some(&mut report));
    let stages: Vec<MessagePipelineStage> = report.steps.iter().map(|s| s.stage).collect();
    assert!(
        stages.contains(&MessagePipelineStage::AfterTrimByCharBudget),
        "budget>0 且超长时应出现 AfterTrimByCharBudget: {:?}",
        stages
    );
    assert!(
        stages.contains(&MessagePipelineStage::AfterSecondCompressTool),
        "budget>0 时应出现第二次 compress 阶段: {:?}",
        stages
    );
}

#[test]
fn drop_orphan_after_trim_count_in_full_pipeline() {
    // 条数裁剪后尾部以 `tool`+`user` 开头，前面的 `assistant+tool_calls` 被裁掉 → 孤立 tool 须由管道剔除。
    let mut v = vec![
        Message::system_only("s"),
        Message::user_only("old"),
        assistant_with_tool_calls(),
        tool_msg("t1"),
        Message::user_only("last"),
    ];
    let cfg = MessagePipelineConfig {
        tool_message_max_chars: 512,
        max_message_history: 2,
        context_char_budget: 0,
        context_min_messages_after_system: 1,
    };
    apply_session_sync_pipeline_with_config(&mut v, cfg, None);
    assert!(
        !v.iter().any(|m| m.role == "tool"),
        "trim 后 tool 无有效前驱时应被 drop_orphan 剔除: {:?}",
        v.iter().map(|m| m.role.as_str()).collect::<Vec<_>>()
    );
    assert!(
        v.iter().any(|m| m.role == "user"
            && crabmate_types::message_content_as_str(&m.content) == Some("last")),
        "应保留尾部 user: {:?}",
        v
    );
}
