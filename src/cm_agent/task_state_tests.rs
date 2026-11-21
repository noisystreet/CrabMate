//! `cm_agent::task_state` 的单元测试。
//!
//! 从 `task_state.rs` 搬出：该文件已顶到 `file ≤ 920` 门禁（908 行），
//! 测试占其中 350 行。搬到独立文件后本文件的实现部分回到 ~550 行，
//! 后续 `#1455` C 步加代码不再撞线。**只搬不改**——内容逐字保留。

    use super::*;

    /// 注入摘要：有界、只含指针、只列**最新**证据。
    #[test]
    fn task_brief_is_bounded_and_carries_no_output_text() {
        let evidence = (1..=10)
            .map(|i| TaskEvidenceRef {
                id: format!("ev_{i}"),
                kind: "verify".to_string(),
                tool: "run_command".to_string(),
                ok: i % 2 == 0,
                exit_code: Some(i),
            })
            .collect();
        let mut s = TaskState::new_paused(
            "s1",
            "把构建修好并跑通测试",
            TaskPauseReason::BudgetTokens,
            TaskBudgetSnapshot::default(),
            evidence,
        );
        s.cumulative = TaskBudgetSnapshot {
            llm_calls: 7,
            outer_loop_iterations: 9,
            estimated_tokens: 12_345,
            elapsed_secs: 88,
        };
        let brief = render_task_brief(&s);
        assert!(
            brief.chars().count() <= TASK_BRIEF_MAX_CHARS,
            "超出上界：{}",
            brief.chars().count()
        );
        assert!(brief.contains("把构建修好并跑通测试"));
        assert!(brief.contains("budget_tokens"), "{brief}");
        assert!(brief.contains("12345"), "应含累计 tokens：{brief}");
        // 只列最新 3 条（ev_8/9/10），旧证据不出现。
        assert!(brief.contains("ev_8") && brief.contains("ev_9") && brief.contains("ev_10"));
        assert!(!brief.contains("ev_7"), "只取最新 {TASK_BRIEF_EVIDENCE_ITEMS} 条：{brief}");
    }

    /// 注入条幂等：重复同步只保留一条；`None` 清空旧条。
    #[test]
    fn sync_keeps_single_injection_and_clears_on_none() {
        let mut messages = vec![crate::cm_types::Message::user_only("hi")];
        sync_paused_task_brief_user_message(&mut messages, Some("摘要A"));
        sync_paused_task_brief_user_message(&mut messages, Some("摘要B"));
        assert_eq!(messages.len(), 2, "重复同步不应堆叠注入条");
        assert!(crate::cm_types::is_paused_task_injection(&messages[1]));
        sync_paused_task_brief_user_message(&mut messages, None);
        assert_eq!(messages.len(), 1, "None 应清空旧注入条");
    }

    /// `#1455` C 步：自动续跑指令同样属「运行时刷新类」——落盘前必须被剥离。
    ///
    /// 理由：它每个续跑回合重新生成，落盘副本既无意义，又会把「机器代发的指令」
    /// 写进会话历史（用户回看时看到一串「继续」）。
    #[test]
    fn auto_resume_injection_is_stripped_before_persist() {
        let mut messages = vec![crate::cm_types::Message::user_only("hi")];
        messages.push(crate::cm_types::Message {
            role: "user".to_string(),
            content: Some(
                crate::cm_types::CRABMATE_AUTO_RESUME_TEXT
                    .to_string()
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(crate::cm_types::CRABMATE_AUTO_RESUME_NAME.to_string()),
            tool_call_id: None,
        });
        assert!(crate::cm_types::is_auto_resume_injection(&messages[1]));
        assert!(
            crate::cm_types::server_injected_user::is_runtime_refreshed_injection(&messages[1]),
            "应归入运行时刷新类"
        );
        // 不算真实用户发言（不进 branch 截断计数）。
        assert!(!crate::cm_types::user_message_counts_for_branch_truncation(&messages[1]));

        crate::cm_types::server_injected_user::strip_runtime_refreshed_injections(&mut messages);
        assert_eq!(messages.len(), 1, "落盘剥离后不应残留续跑指令");
    }

    /// 判据 2 的护栏：落盘前必须能剥离这条注入。
    #[test]
    fn runtime_refreshed_strip_removes_paused_brief() {
        let mut messages = vec![crate::cm_types::Message::user_only("hi")];
        sync_paused_task_brief_user_message(&mut messages, Some("摘要"));
        assert_eq!(messages.len(), 2);
        crate::cm_types::server_injected_user::strip_runtime_refreshed_injections(&mut messages);
        assert_eq!(messages.len(), 1, "落盘剥离后不应残留注入条");
    }

    /// `#1455` C 步的硬闸决策：八种组合各自钉住，安全优先顺序不可颠倒。
    #[test]
    fn auto_resume_decision_covers_every_branch() {
        let base = cfg(0, 0);
        let budget = Some(TaskPauseReason::BudgetTokens);

        // 开关分层：enabled 关就停，即便 auto_resume 想开。
        let mut no_enabled = base.clone();
        no_enabled.auto_resume = true;
        assert_eq!(
            decide_auto_resume(&AutonomyConfig { enabled: false, ..no_enabled }, budget, 0, 0, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::Disabled)
        );
        assert_eq!(
            decide_auto_resume(&base, budget, 0, 0, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::Disabled),
            "auto_resume 默认关"
        );

        let mut on = base.clone();
        on.auto_resume = true;
        // 非预算暂停（正常结束 / 其它失败）不续。
        assert_eq!(
            decide_auto_resume(&on, None, 0, 0, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::NotPaused)
        );
        // 遇审批默认停；放宽后才继续。
        assert_eq!(
            decide_auto_resume(&on, Some(TaskPauseReason::AwaitingApproval), 0, 0, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::AwaitingApproval)
        );
        let mut relaxed = on.clone();
        relaxed.stop_on_approval_required = false;
        assert_eq!(
            decide_auto_resume(&relaxed, Some(TaskPauseReason::AwaitingApproval), 0, 0, false),
            AutoResumeDecision::Go
        );
        // 轮数硬闸：0 = 不限制；达到上限即停。
        assert_eq!(decide_auto_resume(&on, budget, 999, 0, false), AutoResumeDecision::Go);
        let mut capped = on.clone();
        capped.max_autonomous_turns = 3;
        assert_eq!(
            decide_auto_resume(&capped, budget, 2, 0, false),
            AutoResumeDecision::Go
        );
        assert_eq!(
            decide_auto_resume(&capped, budget, 3, 0, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::MaxTurnsReached)
        );
        // 工作区 / 权限变化即停。
        assert_eq!(
            decide_auto_resume(&on, budget, 0, 0, true),
            AutoResumeDecision::Stop(AutoResumeStopReason::ContextChanged)
        );
        // 全绿灯。
        assert_eq!(decide_auto_resume(&on, budget, 0, 0, false), AutoResumeDecision::Go);
    }

    /// `#1455` C 步：连续同因暂停达阈值即停（提前止损），且**轮数硬闸优先于它**。
    ///
    /// 两个闸约束的东西不同：轮数是「还能不能续」，无进展是「续了有没有用」。
    /// 同时满足时报硬闸——绝对上限不该被效率止损掩盖。
    #[test]
    fn no_progress_stops_before_max_turns_but_yields_to_it() {
        let mut on = cfg(0, 0);
        on.auto_resume = true;
        let budget = Some(TaskPauseReason::BudgetTokens);

        // 阈值 2：连续 1 次不停，2 次停。
        assert_eq!(
            decide_auto_resume(&on, budget, 0, 1, false),
            AutoResumeDecision::Go
        );
        assert_eq!(
            decide_auto_resume(&on, budget, 0, 2, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::NoProgress)
        );

        // 轮数硬闸优先。
        let mut capped = on.clone();
        capped.max_autonomous_turns = 1;
        assert_eq!(
            decide_auto_resume(&capped, budget, 1, 5, false),
            AutoResumeDecision::Stop(AutoResumeStopReason::MaxTurnsReached)
        );
    }

    fn cfg(max_tasks: usize, ttl_secs: u64) -> AutonomyConfig {
        AutonomyConfig {
            enabled: true,
            max_tasks,
            task_ttl_secs: ttl_secs,
            max_cumulative_llm_calls: 0,
            max_cumulative_tokens: 0,
            max_cumulative_secs: 0,
            auto_resume: false,
            max_autonomous_turns: 0,
            stop_on_approval_required: true,
        }
    }

    fn budget() -> TaskBudgetSnapshot {
        TaskBudgetSnapshot {
            llm_calls: 7,
            outer_loop_iterations: 4,
            estimated_tokens: 12_345,
            elapsed_secs: 61,
        }
    }

    fn state(scope: &str) -> TaskState {
        TaskState::new_paused(
            scope,
            "修好 CI 并跑通全量测试",
            TaskPauseReason::BudgetWallClock,
            budget(),
            vec![],
        )
    }

    /// 默认配置必须**关闭**：本步只落盘，但落盘本身是新的对外可观察行为（工作区里多出文件），
    /// 因此默认值不是「顺手开」，而是硬约束。
    #[test]
    fn default_config_keeps_autonomy_off() {
        let cfg = crate::cm_config::load_config_test_env::without_cm_env_overrides(|| {
            crate::cm_config::load_config(None).expect("embed default config")
        });
        assert!(!cfg.autonomy.enabled);
        assert_eq!(cfg.autonomy.max_tasks, 200);
        assert_eq!(cfg.autonomy.task_ttl_secs, 604_800);
    }

    #[test]
    fn status_and_reason_strings_are_stable() {
        assert_eq!(TaskStatus::Paused.as_str(), "paused");
        assert_eq!(
            TaskPauseReason::BudgetWallClock.as_str(),
            "budget_wall_clock"
        );
        assert_eq!(TaskPauseReason::BudgetLlmCalls.as_str(), "budget_llm_calls");
        assert_eq!(TaskPauseReason::BudgetTokens.as_str(), "budget_tokens");
    }

    #[test]
    fn pause_reason_maps_every_deny_variant() {
        assert_eq!(
            TaskPauseReason::from(TurnBudgetDeny::WallClock),
            TaskPauseReason::BudgetWallClock
        );
        assert_eq!(
            TaskPauseReason::from(TurnBudgetDeny::LlmCalls),
            TaskPauseReason::BudgetLlmCalls
        );
        assert_eq!(
            TaskPauseReason::from(TurnBudgetDeny::Tokens),
            TaskPauseReason::BudgetTokens
        );
    }

    #[test]
    fn scope_cannot_escape_tasks_dir() {
        for raw in ["../../etc/passwd", "/etc/passwd", "a/b", "..\\x", "a.b"] {
            let name = sanitize_scope(raw);
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains('.'),
                "unsafe file name for {raw}: {name}"
            );
        }
        assert_eq!(sanitize_scope(""), "unknown");
        assert_eq!(sanitize_scope("conv-1_ok"), "conv-1_ok");
    }

    #[test]
    fn goal_truncation_is_unicode_safe() {
        let s = "äöü中文🙂abcdef";
        let t = truncate_chars(s, 5);
        assert_eq!(t.chars().count(), 5, "{t}");
        assert!(t.ends_with('…'), "{t}");
        // 未超上限时原样返回，不加省略号。
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("abc", 0), "");
    }

    #[test]
    fn evidence_keeps_the_latest_items_when_capped() {
        let total = MAX_EVIDENCE_ITEMS + 5;
        let items: Vec<TaskEvidenceRef> = (0..total)
            .map(|i| TaskEvidenceRef {
                id: format!("ev_{i}"),
                kind: "verify".into(),
                tool: "cargo_test".into(),
                ok: true,
                exit_code: Some(0),
            })
            .collect();
        let s = TaskState::new_paused(
            "conv",
            "goal",
            TaskPauseReason::BudgetTokens,
            budget(),
            items,
        );
        assert_eq!(s.evidence.len(), MAX_EVIDENCE_ITEMS);
        // 丢弃的是**最早**的 5 条，最后一条必须是最新证据。
        assert_eq!(s.evidence[0].id, "ev_5");
        assert_eq!(s.evidence[MAX_EVIDENCE_ITEMS - 1].id, format!("ev_{}", total - 1));
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(0, 0));
        let s = state("conv-42");
        let path = store.save(&s).expect("save");
        assert!(path.exists(), "state file should exist");
        let back = store.load("conv-42").expect("load").expect("some");
        assert_eq!(back, s);
    }

    #[test]
    fn save_overwrites_the_same_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(0, 0));
        store.save(&state("conv-1")).expect("first");
        let mut second = state("conv-1");
        second.pause_reason = TaskPauseReason::BudgetTokens;
        store.save(&second).expect("second");
        let files = std::fs::read_dir(store.tasks_dir())
            .expect("dir")
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some(FILE_EXT))
            .count();
        assert_eq!(files, 1, "same scope must reuse one file");
        let back = store.load("conv-1").expect("load").expect("some");
        assert_eq!(back.pause_reason, TaskPauseReason::BudgetTokens);
    }

    #[test]
    fn load_missing_or_unknown_schema_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(0, 0));
        assert!(store.load("nope").expect("load").is_none());

        let path = store.path_for("conv-9");
        std::fs::create_dir_all(store.tasks_dir()).expect("dir");
        let mut v = serde_json::to_value(state("conv-9")).expect("value");
        v["schema"] = serde_json::json!(TASK_STATE_SCHEMA + 1);
        std::fs::write(&path, serde_json::to_vec(&v).expect("bytes")).expect("write");
        assert!(
            store.load("conv-9").expect("load").is_none(),
            "unknown schema must not be guessed"
        );

        std::fs::write(&path, b"{not json").expect("write garbage");
        assert!(
            store.load("conv-9").expect("load").is_none(),
            "unparsable file must be treated as absent"
        );
    }

    #[test]
    fn prune_enforces_max_tasks_oldest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(2, 0));
        for scope in ["a", "b", "c"] {
            store.save(&state(scope)).expect("save");
            // 保证 mtime 有可区分的先后（文件系统时间粒度可能较粗）。
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let remaining: Vec<String> = std::fs::read_dir(store.tasks_dir())
            .expect("dir")
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        assert_eq!(remaining.len(), 2, "{remaining:?}");
        assert!(!remaining.contains(&"a.json".to_string()), "{remaining:?}");
    }

    #[test]
    fn prune_drops_expired_by_ttl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(0, 1));
        let path = store.save(&state("old")).expect("save");
        // 把 mtime 拨回 10 分钟前。
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let f = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open");
        f.set_modified(old).expect("set mtime");
        drop(f);
        let removed = store.prune().expect("prune");
        assert_eq!(removed, 1);
        assert!(!path.exists());
    }

    #[test]
    fn zero_limits_disable_pruning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TaskStateStore::new(dir.path(), &cfg(0, 0));
        store.save(&state("keep")).expect("save");
        assert_eq!(store.prune().expect("prune"), 0);
        assert!(store.load("keep").expect("load").is_some());
    }

    #[test]
    fn serialized_state_has_no_free_text_output() {
        // 不变量 1 的机器化检查：结构里不该出现 detail / stdout 这类字段。
        let s = state("conv");
        let json = serde_json::to_string(&s).expect("json");
        for banned in ["detail", "stdout", "stderr", "content"] {
            assert!(!json.contains(banned), "banned field leaked: {banned}");
        }
        assert_eq!(s.schema, TASK_STATE_SCHEMA);
    }
