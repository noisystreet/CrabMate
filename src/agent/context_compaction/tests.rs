//! `context_compaction` 的单元测试。
//!
//! 从 `mod tests` 拆出（#1347 S2），让 `context_compaction.rs` 守住单文件 920 行上限；
//! golden 另在 `golden.rs`，两者分工：这里守精确水位与守卫语义，那里守决策结构。

use super::*;
use crate::agent::message_pipeline::MIN_RECENT_TURN_GROUPS;
use crate::types::{FunctionDef, Tool};

#[test]
fn format_for_log_reports_budget_and_compaction_counters() {
    let report = ContextCompactionReport {
        budget: Some(ContextTokenBudget {
            context_window_tokens: 128_000,
            max_input_tokens: 117_952,
            trigger_tokens: 70_771,
            target_tokens: 58_976,
            ..Default::default()
        }),
        before: ContextTokenEstimate { used_input_tokens: 120_000, ..Default::default() },
        after: ContextTokenEstimate {
            used_input_tokens: 58_000,
            tool_schema_tokens: 5_800,
            ..Default::default()
        },
        removed_turn_groups: 3,
        removed_messages: 9,
        token_triggered: true,
        summarized_for_token_budget: true,
        compressed_old_tool_messages: 2,
        provider_input_tokens: Some(57_500),
        ..Default::default()
    };
    let line = report.format_for_log();
    for needle in [
        "context_window_tokens=128000",
        "before_tokens=120000",
        "after_tokens=58000",
        "max_input_tokens=117952",
        // `Some(0)` 必须打成 `0`，不能与「未配置」的 `-` 混同（`opt_num` 存在的全部理由）。
        "reserved_output_tokens=0",
        "tool_schema_tokens=5800",
        "removed_turn_groups=3",
        "removed_messages=9",
        "compressed_old_tool_messages=2",
        "token_triggered=true",
        "provider_input_tokens=57500",
        "reason=token_budget_summary",
    ] {
        assert!(line.contains(needle), "缺少 {needle}：{line}");
    }
}

#[test]
fn format_for_log_keeps_usage_without_budget() {
    // 字符预算降级路径（`llm_context_tokens = 0`）：没有 budget，用量仍必须可见，
    // 否则「字符裁剪到底裁掉了多少」在日志里无从判断。
    let report = ContextCompactionReport {
        budget: None,
        after: ContextTokenEstimate { used_input_tokens: 4_096, ..Default::default() },
        ..Default::default()
    };
    let line = report.format_for_log();
    assert!(line.contains("after_tokens=4096"), "{line}");
    assert!(line.contains("max_input_tokens=-"), "{line}");
    assert!(line.contains("provider_input_tokens=-"), "{line}");
    assert!(line.contains("reason=none"), "{line}");
}

fn tool(name: &str, description_len: usize) -> Tool {
    Tool {
        typ: "function".to_string(),
        function: FunctionDef {
            name: name.to_string(),
            description: "d".repeat(description_len),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}}
            }),
        },
    }
}

#[test]
fn tool_schema_and_attachments_are_counted() {
    let cfg = crate::config::load_config(None).expect("embedded config");
    let plain = vec![Message::user_only("hello")];
    let with_image = vec![Message {
        role: "user".to_string(),
        content: Some(MessageContent::Parts(vec![
            serde_json::json!({"type": "text", "text": "hello"}),
            serde_json::json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AA=="}}),
        ])),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }];
    let no_tools = estimate_final_request_tokens(&cfg, &plain, &[], None);
    let rich = estimate_final_request_tokens(&cfg, &with_image, &[tool("read_file", 200)], None);
    assert!(rich.tool_schema_tokens > 0);
    assert_eq!(rich.attachment_tokens, IMAGE_TOKEN_ESTIMATE);
    assert!(rich.used_input_tokens > no_tools.used_input_tokens);
}

#[test]
fn safe_input_budget_reserves_output_and_configured_margin() {
    let cfg = crate::config::load_config(None).expect("embedded config");
    let budget = ContextTokenBudget::from_config(&cfg).expect("token budget enabled");
    assert_eq!(budget.reserved_output_tokens, 16_384);
    assert_eq!(budget.safety_margin_tokens, 2_048);
    assert_eq!(budget.max_input_tokens, 243_712);
    assert_eq!(budget.trigger_tokens, 207_155);
    assert_eq!(budget.target_tokens, 170_598);
}

#[test]
fn token_compaction_removes_whole_old_groups_and_keeps_recent_groups() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    let mut messages = vec![Message::system_only("system")];
    for turn in 0..4 {
        messages.push(Message::user_only(format!("turn-{turn} {}", "x".repeat(8_000))));
        messages.push(Message::assistant_only(format!("answer-{turn}")));
    }
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    let report =
        compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
    assert!(report.removed_turn_groups > 0);
    assert_eq!(conversation_turn_groups(&messages).len(), 2);
    assert!(
        messages.iter().any(|message| {
            crate::types::message_content_as_str(&message.content)
                .is_some_and(|content| content.starts_with("turn-3"))
        }),
        "latest user group must remain"
    );
}

/// 摘要保留的尾部**本身**就超目标时，必须放开尾部保护继续删组。
///
/// 摘要后的视图只剩「摘要 + 尾部」，第 1 级的 `tail_guard` 守卫恒成立、一次都不删，
/// 若没有第 2 级，请求会带着超预算的 messages 发出并被上游拒绝。
#[test]
fn summary_tail_guard_yields_when_tail_alone_exceeds_target() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    cfg.context_pipeline.context_token_safety_margin_tokens = 128;
    cfg.context_pipeline.context_token_trigger_percent = 50;
    cfg.context_pipeline.context_token_target_percent = 40;

    let big = bulky_text("tail", 800);
    let mut messages = vec![
        Message::system_only("system"),
        Message::user_context_summary_injection("压缩要点：早期对话已折叠"),
    ];
    for turn in 0..3 {
        messages.push(Message::user_only(format!("u{turn} {big}")));
        messages.push(Message::assistant_only(format!("a{turn}")));
    }
    let budget = ContextTokenBudget::from_config(&cfg).expect("token budget");
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    assert!(
        before.used_input_tokens > budget.target_tokens,
        "夹具须让摘要尾部自身就超目标: before={} target={}",
        before.used_input_tokens,
        budget.target_tokens
    );

    // 6 = 摘要刻意保留的尾部条数（3 组 × 2 条）：第 1 级守卫必然挡住删组。
    let report =
        compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, true, 6);

    assert!(
        report.removed_turn_groups > 0,
        "尾部自身超目标时必须放开 tail_guard 继续删组"
    );
    assert_eq!(
        conversation_turn_groups(&messages).len(),
        MIN_RECENT_TURN_GROUPS,
        "仍须保留最近两整组"
    );
}

/// 未做摘要（`min_tail_messages == 0`）时，第 2 级不得引起额外删除。
#[test]
fn zero_tail_guard_does_not_double_remove() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    let mut messages = vec![Message::system_only("system")];
    for turn in 0..4 {
        messages.push(Message::user_only(format!(
            "turn-{turn} {}\n",
            bulky_text("x", 600)
        )));
        messages.push(Message::assistant_only(format!("answer-{turn}")));
    }
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    let report =
        compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
    assert_eq!(
        conversation_turn_groups(&messages).len(),
        MIN_RECENT_TURN_GROUPS
    );
    assert_eq!(
        report.removed_turn_groups, 2,
        "4 组删到最近 2 组，第 2 级不能多删"
    );
}

#[test]
fn small_tool_dense_history_does_not_compact_by_message_count() {
    let cfg = crate::config::load_config(None).expect("embedded config");
    let mut messages = vec![Message::system_only("system")];
    for turn in 0..4 {
        messages.push(Message::user_only(format!("turn-{turn}")));
        messages.push(Message::assistant_only("calling tool"));
        messages.push(Message::tool_result_named(format!("call-{turn}"), "read_file".to_string(), "ok"));
        messages.push(Message::assistant_only("done"));
    }
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    let report =
        compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
    assert_eq!(report.removed_turn_groups, 0);
    assert_eq!(conversation_turn_groups(&messages).len(), 4);
}

/// 生成高熵正文：避免重复字符被 BPE 合并成超长 token，保证 Token 估算与真实体积接近。
fn bulky_text(tag: &str, lines: usize) -> String {
    (0..lines)
        .map(|i| format!("第{i}行 {tag} 内容 value_{i}\n"))
        .collect()
}

fn tool_call_message() -> Message {
    Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("calling".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: Some(vec![crate::types::ToolCall {
            id: "call-1".to_string(),
            typ: "function".to_string(),
            function: crate::types::FunctionCall {
                name: "run_command".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        name: None,
        tool_call_id: None,
    }
}

fn tool_message(content: String) -> Message {
    Message::tool_result_named("call-1".to_string(), "run_command".to_string(), content)
}

/// 分级释放 ①：只压缩最近两组之外的 `tool` 正文。
#[test]
fn degrade_compresses_old_tool_bodies_only() {
    let big = bulky_text("tool", 2_000);
    let mut messages = vec![Message::system_only("system")];
    for turn in 0..3 {
        messages.push(Message::user_only(format!("turn-{turn}")));
        messages.push(tool_call_message());
        messages.push(tool_message(big.clone()));
        messages.push(Message::assistant_only("done"));
    }
    let compressed = compress_old_tool_messages(
        &mut messages,
        MIN_RECENT_TURN_GROUPS,
        DEGRADE_TOOL_MAX_CHARS,
    );
    assert_eq!(compressed, 1, "仅最旧交互组的 tool 正文应被二次压缩");
    let lengths: Vec<usize> = messages
        .iter()
        .filter(|message| message.role == "tool")
        .map(|message| {
            crate::types::message_content_as_str(&message.content)
                .unwrap_or("")
                .chars()
                .count()
        })
        .collect();
    assert_eq!(lengths.len(), 3);
    assert!(lengths[0] < big.chars().count(), "旧组正文应被压缩");
    assert_eq!(lengths[1], big.chars().count(), "最近两组保持原样");
    assert_eq!(lengths[2], big.chars().count(), "最近两组保持原样");
}

/// 超预算时优先压旧 `tool` 正文；压缩后达标则不再删组。
#[test]
fn token_compaction_prefers_tool_body_degrade_over_group_removal() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 16_384;
    cfg.llm_sampling.max_tokens = 512;
    let mut messages = vec![Message::system_only("system")];
    messages.push(Message::user_only("turn-0"));
    messages.push(tool_call_message());
    messages.push(tool_message(bulky_text("tool", 4_000)));
    messages.push(Message::assistant_only("done"));
    for turn in 1..3 {
        messages.push(Message::user_only(format!("turn-{turn}")));
        messages.push(Message::assistant_only("answer"));
    }
    let before = estimate_final_request_tokens(&cfg, &messages, &[], None);
    let report =
        compact_messages_to_token_budget(&cfg, &mut messages, &[], None, before, false, 0);
    assert!(report.token_triggered, "巨型 tool 正文应触发 Token 预算");
    assert_eq!(report.compressed_old_tool_messages, 1);
    assert_eq!(report.removed_turn_groups, 0, "压缩旧 tool 正文后应已达标");
    let target = report.budget.expect("token budget enabled").target_tokens;
    assert!(report.after.used_input_tokens <= target);
}

/// 第 1 级（软目标）必须尊重摘要保留的尾部：保护范围覆盖全部消息时一组都不删。
///
/// 这里直接驱动单级函数：端到端的**撞硬上限放开**由
/// `summary_tail_guard_yields_when_tail_alone_exceeds_target` 覆盖。
#[test]
fn min_tail_messages_stops_group_removal() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    let budget = ContextTokenBudget::from_config(&cfg).expect("token budget");
    let build = || {
        let mut messages = vec![Message::system_only("system")];
        for turn in 0..4 {
            messages.push(Message::user_only(bulky_text(&format!("turn{turn}"), 400)));
            messages.push(Message::assistant_only("answer"));
        }
        messages
    };

    let mut guarded_messages = build();
    let before = estimate_final_request_tokens(&cfg, &guarded_messages, &[], None);
    let mut guarded = GroupRemovalOutcome {
        after: before,
        ..GroupRemovalOutcome::default()
    };
    let converged = remove_groups_until_target(
        &mut guarded,
        &cfg,
        &mut guarded_messages,
        &[],
        None,
        RemovalStep {
            target_tokens: budget.target_tokens,
            tail_guard: 100,
            min_groups: MIN_RECENT_TURN_GROUPS,
        },
    );
    assert!(!converged, "尾部保护覆盖全部消息时不可能收敛到软目标");
    assert_eq!(guarded.turn_groups, 0, "尾部保护应阻止删组");

    let mut free_messages = build();
    let before = estimate_final_request_tokens(&cfg, &free_messages, &[], None);
    let freed =
        compact_messages_to_token_budget(&cfg, &mut free_messages, &[], None, before, false, 0);
    assert!(freed.removed_turn_groups > 0, "无尾部保护时应删组");
}

/// 生产默认尾部条数（12）下，第 1 级保护让保留的组多于无保护情形。
#[test]
fn min_tail_messages_default_tail_keeps_more_groups_than_no_guard() {
    let mut cfg = crate::config::load_config(None).expect("embedded config");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    let tail = cfg
        .context_pipeline
        .context_summary_tail_messages
        .clamp(4, 64);
    assert_eq!(tail, 12, "默认配置的摘要尾部条数");
    let budget = ContextTokenBudget::from_config(&cfg).expect("token budget");
    let build = || {
        let mut messages = vec![Message::system_only("system")];
        for turn in 0..8 {
            messages.push(Message::user_only(bulky_text(&format!("turn{turn}"), 200)));
            messages.push(Message::assistant_only("answer"));
        }
        messages
    };

    let mut guarded_messages = build();
    let before = estimate_final_request_tokens(&cfg, &guarded_messages, &[], None);
    let mut guarded = GroupRemovalOutcome {
        after: before,
        ..GroupRemovalOutcome::default()
    };
    let _ = remove_groups_until_target(
        &mut guarded,
        &cfg,
        &mut guarded_messages,
        &[],
        None,
        RemovalStep {
            target_tokens: budget.target_tokens,
            tail_guard: tail,
            min_groups: MIN_RECENT_TURN_GROUPS,
        },
    );

    let mut free_messages = build();
    let before_free = estimate_final_request_tokens(&cfg, &free_messages, &[], None);
    let freed = compact_messages_to_token_budget(
        &cfg,
        &mut free_messages,
        &[],
        None,
        before_free,
        false,
        0,
    );

    assert!(guarded.turn_groups > 0, "预算超限时应至少删一组");
    assert!(
        freed.removed_turn_groups > guarded.turn_groups,
        "无尾部保护应删得更多: guarded={} freed={}",
        guarded.turn_groups,
        freed.removed_turn_groups
    );
    assert!(
        guarded_messages.len() >= tail,
        "保护生效后应保留至少 {tail} 条消息: {}",
        guarded_messages.len()
    );
}

/// ADR §3.3 的不变式：`DEGRADE_TOOL_MAX_CHARS` 必须**显著小于** `tool_message_max_chars`。
///
/// 反了会静默失效：同步管道先按 `tool_message_max_chars` 压过一遍，正文若已在该值以下，
/// `maybe_compress_tool_message_content` 返回 `None`，分级释放 ① 永不命中——而压缩照样
/// 「看起来在工作」（删组仍生效），只有 Token 水位会莫名偏高。policy 单测里的
/// `max_chars: DEGRADE_TOOL_MAX_CHARS` 是**自比**（改常量不会红），所以这条单独钉一次。
#[test]
fn degrade_threshold_stays_far_below_tool_body_cap() {
    let cfg = crate::config::load_config(None).expect("embedded config");
    let cap = cfg.tool_transcript.tool_message_max_chars;
    assert!(
        cap >= DEGRADE_TOOL_MAX_CHARS * 8,
        "tool_message_max_chars={cap} 应远大于 DEGRADE_TOOL_MAX_CHARS={DEGRADE_TOOL_MAX_CHARS}，\
         否则分级释放 ① 永不命中"
    );
}
