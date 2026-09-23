use super::*;
use std::time::Duration;

// 独立子模块（registry.rs 顶层 `#[path]` include）：与 `mod streaming` / `mod finished_sink` 同源。
fn limits() -> JobLimits {
    JobLimits {
        max_concurrent: 2,
        max_queued: 2,
        ttl: Duration::from_secs(3600),
        grace: Duration::from_secs(60),
        max_entries: 4,
        output_buffer_bytes: 262_144,
    }
}

fn job_id(r: &JobRecord) -> String {
    r.id.clone()
}

fn spawn_default() -> JobSpawn {
    JobSpawn {
        program: "true".to_string(),
        args: Vec::new(),
        cwd: PathBuf::from("/"),
        extra_env: Vec::new(),
        wall: Duration::from_secs(10),
        max_output_len: 1024,
    }
}

fn register_default(reg: &ToolJobRegistry) -> String {
    reg.register(
        PathBuf::from("/w"),
        None,
        spawn_default(),
        r#"{"command":"true"}"#.to_string(),
        None,
    )
    .expect("register")
}

#[test]
fn register_queues_and_get_returns_record() {
    let reg = ToolJobRegistry::new(limits());
    let id = reg
        .register(
            PathBuf::from("/ws"),
            Some(7),
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        )
        .expect("register");
    assert!(id.starts_with("tooljob_"), "id: {id}");
    assert_eq!(id.len(), "tooljob_".len() + 32);
    let rec = reg.get(&id).expect("record");
    assert_eq!(rec.status, JobStatus::Queued);
    assert_eq!(rec.workspace, PathBuf::from("/ws"));
    assert_eq!(rec.source_turn_job_id, Some(7));
}

#[test]
fn list_newest_first_filters_workspace_and_truncates() {
    let reg = ToolJobRegistry::new(limits());
    let ws = std::path::Path::new("/ws");
    let register_at = |workspace: &str| {
        let id = reg
            .register(
                PathBuf::from(workspace),
                None,
                spawn_default(),
                r#"{"command":"true"}"#.to_string(),
                None,
            )
            .expect("register");
        // `created_at` 取 `SystemTime::now()`，退避 1ms 以保证倒序断言确定性。
        std::thread::sleep(Duration::from_millis(1));
        id
    };
    let first = register_at("/ws");
    let second = register_at("/ws");
    let other = register_at("/other");

    let ids = |rows: Vec<JobRecord>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(reg.list(Some(ws), 10)), vec![second.clone(), first.clone()]);
    assert_eq!(
        ids(reg.list(None, 10)),
        vec![other.clone(), second.clone(), first.clone()]
    );
    assert_eq!(ids(reg.list(None, 2)), vec![other.clone(), second.clone()]);
    assert_eq!(ids(reg.list(Some(std::path::Path::new("/other")), 10)), vec![
        other.clone()
    ]);
    assert!(reg.list(None, 0).is_empty());
}

#[test]
fn cancel_non_terminal_for_source_turn_skips_other_turns() {
    let reg = ToolJobRegistry::new(limits());
    let id_keep = reg
        .register(
            PathBuf::from("/ws"),
            Some(1),
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        )
        .expect("keep");
    let id_stop = reg
        .register(
            PathBuf::from("/ws"),
            Some(2),
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        )
        .expect("stop");
    assert_eq!(reg.cancel_non_terminal_for_source_turn(2), 1);
    assert_eq!(reg.get(&id_stop).expect("stop").status, JobStatus::Cancelled);
    assert_eq!(reg.get(&id_keep).expect("keep").status, JobStatus::Queued);
}

#[test]
fn try_start_fifo_and_respects_max_concurrent() {
    let reg = ToolJobRegistry::new(limits());
    let a = register_default(&reg); // a
    let b = register_default(&reg); // b
    let c = register_default(&reg); // 第 3 个：进入队列
    let r1 = reg.try_start().expect("r1");
    let r2 = reg.try_start().expect("r2");
    assert_eq!(job_id(&r1), a);
    assert_eq!(job_id(&r2), b);
    assert!(reg.try_start().is_none(), "并发满，不得再领取");
    assert_eq!(reg.get(&c).expect("c").status, JobStatus::Queued);
    assert_eq!(reg.stats().running, 2);
    assert_eq!(reg.stats().queued, 1);
}

#[test]
fn register_rejects_when_queue_full() {
    let reg = ToolJobRegistry::new(JobLimits {
        max_entries: 100,
        ..limits()
    }); // concurrent=2, queued=2；entries 上限放大以免先触发 AtCapacity
    register_default(&reg); // a
    register_default(&reg); // b
    reg.try_start().expect("r1");
    reg.try_start().expect("r2"); // 并发已满
    register_default(&reg); // c
    register_default(&reg); // 队列已满
    assert_eq!(
        reg.register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        ),
        Err(RegisterError::QueueFull)
    );
}

#[test]
fn cancel_queued_moves_terminal_and_removes_from_queue() {
    let reg = ToolJobRegistry::new(limits());
    let a = register_default(&reg); // a
    let b = register_default(&reg); // b
    assert_eq!(reg.cancel(&a), CancelOutcome::Cancelled);
    assert_eq!(reg.get(&a).expect("a").status, JobStatus::Cancelled);
    assert!(reg.get(&a).expect("a").finished_at.is_some());
    // 队列不再含 a：领取到的应为 b
    let r = reg.try_start().expect("start");
    assert_eq!(job_id(&r), b);
}

#[test]
fn cancel_running_sets_flag_then_complete_transitions() {
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg); // id
    reg.try_start().expect("start");
    let flag = reg.cancel_flag(&id).expect("cancel flag");
    assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);
    assert!(reg.get(&id).expect("rec").cancel_requested);
    assert!(flag.load(Ordering::SeqCst), "worker 取消信号应被置位");
    assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
    let outcome = JobOutcome {
        status: JobStatus::Cancelled,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: Some("cancelled".into()),
        failure_category: None,
    };
    assert!(reg.complete(&id, outcome, false));
    let rec = reg.get(&id).expect("rec");
    assert_eq!(rec.status, JobStatus::Cancelled);
    assert_eq!(rec.outcome.as_ref().expect("out").error_code.as_deref(), Some("cancelled"));
    assert_eq!(reg.stats().running, 0);
}

#[test]
fn complete_rejects_non_terminal_outcome() {
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg); // id
    reg.try_start().expect("start");
    let running = JobOutcome {
        status: JobStatus::Running,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    assert!(!reg.complete(&id, running, false), "非终态 outcome 不得写入");
    assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
    assert_eq!(reg.stats().running, 1);
}

#[test]
fn complete_rejects_terminal_overwrite_and_unknown() {
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg); // id
    let ok = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: b"out".to_vec(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    assert!(reg.complete(&id, ok.clone(), true));
    assert!(!reg.complete(&id, ok.clone(), false), "终态不可覆盖");
    assert!(!reg.complete("tooljob_missing", ok.clone(), false));
    assert!(reg.get(&id).expect("rec").workspace_changed);
}

#[test]
fn cleanup_removes_only_terminal_past_ttl_and_grace() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg); // id
    let ok = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    reg.complete(&id, ok, false);
    let now = SystemTime::now();
    // 完成但未过 grace：保留
    assert_eq!(reg.cleanup(now), 0);
    // 完成且过 grace（由 `finished_at` 起算），但自创建不足 ttl：仍保留（ttl 自创建算）
    let finished = reg.get(&id).expect("rec").finished_at.expect("finished");
    let later = finished + Duration::from_secs(20);
    assert_eq!(reg.cleanup(later), 0, "ttl 未到不可删");
    // 同时过 ttl 与 grace：删除
    let far = reg.get(&id).expect("rec").created_at + Duration::from_secs(200);
    assert_eq!(reg.cleanup(far), 1);
    assert!(reg.get(&id).is_none());
}

#[test]
fn cleanup_never_removes_running() {
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg); // id
    reg.try_start().expect("start");
    let far = SystemTime::now() + Duration::from_secs(10_000);
    assert_eq!(reg.cleanup(far), 0);
    assert_eq!(reg.get(&id).expect("rec").status, JobStatus::Running);
}

#[test]
fn eviction_only_terminal_and_lowest_created() {
    let reg = ToolJobRegistry::new(JobLimits {
        max_entries: 2,
        ..limits()
    });
    let a = register_default(&reg); // a
    let b = register_default(&reg); // b
    // 无终态可淘汰：AtCapacity
    assert_eq!(
        reg.register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        ),
        Err(RegisterError::AtCapacity)
    );
    // 完成 a（终态）后注册 c → 淘汰 a
    let ok = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    reg.complete(&a, ok, false);
    let c = register_default(&reg); // c
    assert!(reg.get(&a).is_none(), "最旧终态应被淘汰");
    assert!(reg.get(&b).is_some());
    assert!(reg.get(&c).is_some());
}

#[test]
fn gen_id_is_opaque_hex() {
    let a = gen_tool_job_id();
    let b = gen_tool_job_id();
    assert_ne!(a, b);
    assert!(a.starts_with("tooljob_"));
    let hex = a.trim_start_matches("tooljob_");
    assert_eq!(hex.len(), 32);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn get_checked_found_not_found_and_lazy_expiry() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg); // id
    // 非终态（queued）：直接 Found，不过期。
    assert!(matches!(
        reg.get_checked(&id, SystemTime::now() + Duration::from_secs(10_000)),
        GetOutcome::Found(_)
    ));
    // 从未创建：NotFound。
    assert!(matches!(
        reg.get_checked("tooljob_unknown", SystemTime::now()),
        GetOutcome::NotFound
    ));
    // 完成后已过 grace 且自创建过 ttl：惰性判定 Expired 并删除。
    let ok = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    assert!(reg.complete(&id, ok, false));
    let rec = reg.get(&id).expect("rec");
    let expired_at = rec.created_at + Duration::from_secs(200);
    assert!(matches!(reg.get_checked(&id, expired_at), GetOutcome::Expired));
    assert!(reg.get(&id).is_none());
    // 删除后仍能识别为 Expired（而非 NotFound）。
    assert!(matches!(reg.get_checked(&id, expired_at), GetOutcome::Expired));
}

#[test]
fn cleanup_remembers_expired_then_cancel_reports_expired() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg); // id
    let ok = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    reg.complete(&id, ok, false);
    let far = SystemTime::now() + Duration::from_secs(10_000);
    assert_eq!(reg.cleanup(far), 1);
    assert!(reg.get(&id).is_none());
    assert_eq!(reg.cancel(&id), CancelOutcome::Expired);
    assert_eq!(reg.cancel("tooljob_never"), CancelOutcome::NotFound);
    // 被淘汰的终态同样记入 expired。
    let reg2 = ToolJobRegistry::new(JobLimits {
        max_entries: 2,
        ..limits()
    });
    let a = register_default(&reg2); // a
    let b = register_default(&reg2); // b（占用全部条目，无终态可淘汰）
    let ok2 = JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    };
    reg2.complete(&a, ok2, false);
    register_default(&reg2); // 触发淘汰终态 a
    assert!(reg2.get(&b).is_some(), "非终态 b 不可被淘汰");
    assert_eq!(reg2.cancel(&a), CancelOutcome::Expired);
}

#[test]
fn get_checked_does_not_expire_running_or_queued() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(1),
        grace: Duration::from_secs(1),
        ..limits()
    });
    let queued = register_default(&reg); // queued
    let running = register_default(&reg); // running
    reg.try_start().expect("start");
    let far = SystemTime::now() + Duration::from_secs(3600);
    assert!(matches!(reg.get_checked(&queued, far), GetOutcome::Found(_)));
    assert!(matches!(reg.get_checked(&running, far), GetOutcome::Found(_)));
    assert_eq!(reg.stats().total, 2, "非终态不得被惰性清理");
}
