use super::*;

/// 验收辅助：摘要正文是否覆盖给定锚点（路径/错误串等）。
fn context_summary_covers_anchors(summary: &str, anchors: &[&str]) -> bool {
    anchors
        .iter()
        .all(|a| !a.is_empty() && summary.contains(*a))
}

#[test]
fn budget_pressure_tightens_sync_pipeline_char_budget() {
    let mut cfg = crate::config::load_config(None).expect("embed default");
    cfg.context_pipeline.context_char_budget = 20_000;
    cfg.session_ui.max_message_history = 100;
    cfg.tool_transcript.tool_message_max_chars = 1_000_000;
    cfg.llm_sampling.llm_context_tokens = 0;
    cfg.turn_budget.max_turn_tokens = 100;

    let budget = crate::agent::turn_budget::TurnBudgetCounter::new_shared();
    budget.record_estimated_tokens(75);

    let mut loose = vec![Message::system_only("s")];
    let mut tight = loose.clone();
    for i in 0..30 {
        let m = Message::user_only(format!("u{i}: {}", "x".repeat(800)));
        loose.push(m.clone());
        tight.push(m);
    }
    prepare_messages_before_model_call_sync(&mut loose, &cfg);
    prepare_messages_before_model_call_sync_with_budget(&mut tight, &cfg, Some(&budget));
    assert!(
        tight.len() <= loose.len(),
        "budget pressure should trim at least as aggressively"
    );
}

/// 只读 dry-run 入口（旁路提问）：把输入 token 收敛到窗口内，但**不**触发 LLM 摘要，
/// 也不写 canonical / 归档 / artifact（该入口无 store 参数，落盘由调用方另择，属结构性保证）。
#[tokio::test]
async fn readonly_entry_bounds_tokens_without_summary_or_persistence() {
    let mut cfg = crate::config::load_config(None).expect("embed default");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    // 让同步字符/条数裁剪不先行兜底，从而真实检验「token 预算压缩」路径。
    cfg.session_ui.max_message_history = 1_000;
    cfg.context_pipeline.context_char_budget = 1_000_000;
    cfg.tool_transcript.tool_message_max_chars = 1_000_000;
    cfg.context_pipeline.context_summary_trigger_chars = 0;

    let mut messages = vec![Message::system_only("sys")];
    for turn in 0..30 {
        messages.push(Message::user_only(format!("u{turn}: {}", "x".repeat(1_500))));
        messages.push(Message::assistant_only(format!("a{turn}")));
    }
    let budget = crate::agent::context_compaction::ContextTokenBudget::from_config(&cfg)
        .expect("token budget enabled");
    let before = crate::agent::context_compaction::estimate_final_request_tokens(
        &cfg, &messages, &[], None,
    );
    assert!(before.used_input_tokens > budget.trigger_tokens);

    let delta = prepare_readonly_model_messages(&cfg, &mut messages, &[], None, None)
        .await
        .expect("readonly entry");

    assert!(!delta.summarized, "只读入口不得触发 LLM 摘要");
    assert!(delta.summary_tail_kept.is_none());
    assert_eq!(delta.summary_removed_prefix.removed, 0);
    assert!(delta.compaction.token_triggered);
    assert!(delta.compaction.removed_turn_groups > 0);
    assert!(
        delta.compaction.after.used_input_tokens < before.used_input_tokens,
        "只读入口应实际收缩输入: before={} after={}",
        before.used_input_tokens,
        delta.compaction.after.used_input_tokens
    );
    assert!(
        delta.compaction.after.used_input_tokens <= budget.max_input_tokens,
        "只读入口必须把输入 token 收敛到窗口内: after={} max_input={}",
        delta.compaction.after.used_input_tokens,
        budget.max_input_tokens
    );
}

/// ADR §9.2 item7：`/btw` 只读入口**默认不触发 LLM 摘要**——`context_btw_summary_enabled`
/// 内嵌默认关闭；「端到端不落盘」由入口签名结构性保证（无 store 参数，操作的是调用方克隆）。
#[test]
fn btw_readonly_summary_flag_defaults_off() {
    let cfg = crate::config::load_config(None).expect("embed default");
    assert!(
        !cfg.context_pipeline.context_btw_summary_enabled,
        "context_btw_summary_enabled 默认必须为 false（btw 默认不触发摘要）"
    );
}

#[test]
fn summary_tail_starts_at_complete_turn_group_boundary() {
    let mut messages = vec![Message::system_only("s")];
    for turn in 0..4 {
        messages.push(Message::user_only(format!("u{turn}")));
        messages.push(Message::assistant_only(format!("a{turn}")));
        messages.push(Message::assistant_only(format!("done{turn}")));
    }
    let tail = complete_turn_group_tail_len(&messages, 4).expect("four complete groups");
    let start = messages.len() - tail;
    assert_eq!(messages[start].role, "user");
    assert_eq!(
        crate::types::message_content_as_str(&messages[start].content),
        Some("u2")
    );
}

#[test]
fn format_context_summary_user_replaces_placeholders() {
    let out = format_context_summary_user(
        "上限 {max_tokens}\n---\n{transcript}\n---",
        512,
        "user: 修 src/foo.rs\nerror: E0308",
    );
    assert!(out.contains("512"));
    assert!(out.contains("src/foo.rs"));
    assert!(out.contains("E0308"));
    assert!(!out.contains("{max_tokens}"));
    assert!(!out.contains("{transcript}"));
}

#[test]
fn format_context_summary_user_accepts_max_chars_alias() {
    let out = format_context_summary_user("n={max_chars}\n{transcript}", 256, "path.rs");
    assert!(out.contains("n=256"));
    assert!(out.contains("path.rs"));
}

#[test]
fn format_context_summary_user_appends_when_transcript_placeholder_missing() {
    let out = format_context_summary_user("只有骨架无占位", 128, "crates/demo/src/path_bug.rs");
    assert!(out.contains("只有骨架无占位"));
    assert!(out.contains("crates/demo/src/path_bug.rs"));
    assert!(out.contains("对话记录："));
}

#[test]
fn loaded_summary_prompts_require_retention_and_structure() {
    let cfg = crate::config::load_config(None).expect("embed default");
    let sys = &cfg.context_pipeline.context_summary_system;
    for needle in ["必须保留", "禁止编造", "关键路径", "错误信息", "未决"] {
        assert!(
            sys.contains(needle),
            "context_summary_system missing `{needle}`"
        );
    }
    let user_t = &cfg.context_pipeline.context_summary_user_template;
    for needle in [
        "## 目标",
        "## 已完成",
        "## 未决",
        "## 关键路径与错误",
        "{transcript}",
    ] {
        assert!(
            user_t.contains(needle),
            "context_summary_user_template missing `{needle}`"
        );
    }
    assert!(
        user_t.contains("{max_tokens}") || user_t.contains("{max_chars}"),
        "user template should mention a length placeholder"
    );
}

#[test]
fn side_messages_embed_fixture_transcript_anchors() {
    let cfg = crate::config::load_config(None).expect("embed default");
    let transcript = concat!(
        "user: 修复 crates/demo/src/path_bug.rs 的类型错误\n",
        "assistant: [tool_calls] read_file({\"path\":\"crates/demo/src/path_bug.rs\"})\n",
        "tool: error[E0308]: mismatched types in path_bug.rs\n",
    );
    let msgs = build_context_summary_side_messages(&cfg, transcript);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
    let user = crate::types::message_content_as_str(&msgs[1].content).unwrap_or("");
    assert!(
        context_summary_covers_anchors(
            user,
            &[
                "crates/demo/src/path_bug.rs",
                "error[E0308]",
                "## 目标",
                "## 关键路径与错误",
            ]
        ),
        "summary side user must carry transcript anchors and skeleton; got:\n{user}"
    );
}

#[test]
fn fixture_good_summary_covers_path_and_error() {
    // 文档化验收：合格摘要须保留路径与错误锚点（供日后接 mock LLM 评测复用）。
    let good = concat!(
        "## 目标\n修复 path_bug 类型错误\n\n",
        "## 已完成\nread_file crates/demo/src/path_bug.rs\n\n",
        "## 未决\n尚未 patch\n\n",
        "## 关键路径与错误\ncrates/demo/src/path_bug.rs；error[E0308]: mismatched types\n",
    );
    assert!(context_summary_covers_anchors(
        good,
        &["crates/demo/src/path_bug.rs", "error[E0308]"]
    ));
    let bad = "## 目标\n修个文件\n\n## 关键路径与错误\n某个源码里类型不对\n";
    assert!(!context_summary_covers_anchors(
        bad,
        &["crates/demo/src/path_bug.rs", "error[E0308]"]
    ));
}

#[test]
fn apply_llm_summary_writes_named_injection_and_keeps_tail() {
    let mut messages = vec![
        Message::system_only("sys"),
        Message::user_only("old-1"),
        Message::assistant_only("a1"),
        Message::user_only("tail-user"),
        Message::assistant_only("tail-a"),
    ];
    apply_llm_summary_to_messages(&mut messages, 2, "要点：修了 path_bug");
    assert_eq!(messages[0].role, "system");
    assert!(crate::types::is_context_summary_injection(&messages[1]));
    assert_eq!(
        messages[1].name.as_deref(),
        Some(crate::types::CRABMATE_CONTEXT_SUMMARY_NAME)
    );
    assert!(!crate::types::user_message_counts_for_branch_truncation(
        &messages[1]
    ));
    assert_eq!(messages.len(), 4);
    assert_eq!(
        crate::types::message_content_as_str(&messages[2].content),
        Some("tail-user")
    );
    let snap = crate::types::filter_messages_for_web_client_snapshot(&messages);
    assert!(
        snap.iter()
            .all(|m| !crate::types::is_context_summary_injection(m))
    );
    assert!(
        snap.iter()
            .any(|m| crate::types::message_content_as_str(&m.content) == Some("tail-user"))
    );
    let vendor = crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(
        &messages, false, false,
    );
    assert!(
        vendor
            .iter()
            .any(crate::types::is_context_summary_injection),
        "summary must still go to the vendor"
    );
}

#[test]
fn legacy_unnamed_summary_prefix_stays_persisted_and_hidden() {
    let legacy = Message::user_only(format!(
        "{}\n旧会话无 name",
        crate::types::CONTEXT_SUMMARY_INJECTION_CONTENT_PREFIX
    ));
    assert!(crate::types::is_context_summary_injection(&legacy));
    assert!(!crate::types::user_message_counts_for_branch_truncation(
        &legacy
    ));
    let mut v = vec![Message::user_only("真实"), legacy.clone()];
    crate::types::strip_orchestration_injected_users_for_conversation_store(&mut v);
    assert_eq!(v.len(), 2);
    assert_eq!(
        crate::types::filter_messages_for_web_client_snapshot(&v).len(),
        1
    );
}

/// §9.2 多回合夹具：真实回合计序为「先 push 本轮 user 进 canonical → derive → 同步裁剪/估算
/// → token 预算压缩 → push assistant → `commit_current_turn_to`」。无 LLM 摘要（默认 btw/同步
/// 路径），归档恒为被移出的原文前缀（§3.8）。
fn multi_turn_cfg() -> AgentConfig {
    let mut cfg = crate::config::load_config(None).expect("embed default");
    cfg.llm_sampling.llm_context_tokens = 4_096;
    cfg.llm_sampling.max_tokens = 512;
    cfg.context_pipeline.context_token_safety_margin_tokens = 128;
    cfg.context_pipeline.context_token_trigger_percent = 50;
    cfg.context_pipeline.context_token_target_percent = 40;
    // 关闭条数 / 字符裁剪，确保 token 预算压缩是唯一驱动。
    cfg.session_ui.max_message_history = 100_000;
    cfg.tool_transcript.tool_message_max_chars = 1_000_000;
    cfg.context_pipeline.context_char_budget = 0;
    cfg.context_pipeline.context_summary_trigger_chars = 0;
    cfg
}

/// 单回合步进：返回 (本轮压缩前输入 token, 被移出 canonical 的归档前缀)。
fn run_multi_turn_once(
    cfg: &AgentConfig,
    canonical: &mut Vec<Message>,
    turn: usize,
) -> (u32, Vec<Message>) {
    canonical.push(Message::user_only(format!("u{turn}: {}", "x".repeat(2_000))));
    let mut view = crate::agent::model_context_view::ModelContextView::derive(canonical);
    let (_pipeline, before) =
        sync_trim_and_estimate_before(cfg, view.messages_mut(), &[], None, None);
    let report =
        finalize_token_budget_compaction(cfg, view.messages_mut(), &[], None, before, false);
    view.messages_mut().push(Message::assistant_only(format!("a{turn}")));
    let commit = view.commit_current_turn_to(canonical);
    assert!(commit.archived_artifacts.is_empty());
    (report.before.used_input_tokens, commit.archived_prefix)
}

/// §9.2 item1：跨回合收敛 —— 持续触发压缩后，`before_tokens` 不随回合数无界回升。
#[test]
fn multi_turn_compaction_converges_before_tokens() {
    let cfg = multi_turn_cfg();
    let budget =
        crate::agent::context_compaction::ContextTokenBudget::from_config(&cfg).expect("budget");
    let turns = 12usize;
    let mut canonical = vec![Message::system_only("sys")];
    let mut before_tokens: Vec<u32> = Vec::new();
    for turn in 0..turns {
        let (before, _archived) = run_multi_turn_once(&cfg, &mut canonical, turn);
        before_tokens.push(before);
    }
    assert!(
        before_tokens.iter().any(|&t| t > budget.trigger_tokens),
        "夹具应至少触发一次压缩: {before_tokens:?}"
    );
    let per_turn = before_tokens[1].saturating_sub(before_tokens[0]);
    assert!(per_turn > 0, "单回合自然增量应为正: {before_tokens:?}");
    let cap = budget.trigger_tokens + 2 * per_turn;
    for (turn, &before) in before_tokens.iter().enumerate() {
        assert!(
            before <= cap,
            "回合 {turn} 的 before_tokens={before} 超过收敛上界 {cap}（trigger={}, per_turn={per_turn}）: {before_tokens:?}",
            budget.trigger_tokens
        );
    }
}

/// §9.2 item2：canonical 封顶 —— 长会话（≥50 回合）后 canonical 条数稳定在阈值量级，不再单调增长。
#[test]
fn multi_turn_compaction_caps_canonical_length() {
    let cfg = multi_turn_cfg();
    let turns = 60usize;
    let mut canonical = vec![Message::system_only("sys")];
    let mut canonical_lens: Vec<usize> = Vec::new();
    for turn in 0..turns {
        let (_before, _archived) = run_multi_turn_once(&cfg, &mut canonical, turn);
        canonical_lens.push(canonical.len());
    }
    // 未压缩时 canonical.len() == 1 + 2*turns；压缩后应远小于该量级。
    let uncompressed = 1 + 2 * turns;
    let peak = *canonical_lens.iter().max().expect("non-empty");
    assert!(
        peak < turns,
        "canonical 峰值 {peak} 应远小于未压缩规模 {uncompressed}: {canonical_lens:?}"
    );
    // 后半程不再单调增长（稳定在阈值量级）。
    let mid = canonical_lens[turns / 2];
    assert!(
        canonical_lens[turns - 1] <= mid + 4,
        "canonical 后段应稳定: mid={mid} last={}: {canonical_lens:?}",
        canonical_lens[turns - 1]
    );
}

/// §9.2 item10：结构性不变式 —— 「归档 ∪ canonical 原文 = 完整原始历史」逐字成立。
#[test]
fn archive_union_canonical_preserves_full_original_history() {
    let cfg = multi_turn_cfg();
    let turns = 20usize;
    let mut canonical = vec![Message::system_only("sys")];
    let mut archive: Vec<Message> = Vec::new();
    let mut pushed: Vec<Message> = Vec::new();
    for turn in 0..turns {
        let user = Message::user_only(format!("u{turn}: {}", "x".repeat(2_000)));
        let (before, archived) = run_multi_turn_once(&cfg, &mut canonical, turn);
        pushed.push(user);
        // run_multi_turn_once 内部已追加本轮 assistant；此处补记原文序列。
        pushed.push(Message::assistant_only(format!("a{turn}")));
        let _ = before;
        archive.extend(archived);
    }
    assert!(
        !archive.is_empty(),
        "长会话应产生归档前缀: canonical.len()={}",
        canonical.len()
    );
    assert_eq!(canonical[0].role, "system");
    let mut combined = archive;
    combined.extend_from_slice(&canonical[1..]);
    assert_eq!(
        combined, pushed,
        "归档 ∪ canonical[1..] 应与完整原始历史逐字一致"
    );
}
