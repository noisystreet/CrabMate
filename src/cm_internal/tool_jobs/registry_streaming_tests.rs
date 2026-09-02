use super::*;
use std::time::Duration;

// 独立子模块（registry.rs 顶层 `#[path]` include）：与 `mod tests` 同源自测常量，避免跨模块可见性问题。
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
    )
    .expect("register")
}

fn ok_outcome(status: JobStatus) -> JobOutcome {
    JobOutcome {
        status,
        exit_code: None,
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    }
}

#[test]
fn push_output_and_read_incremental_no_gaps_or_dups() {
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg);
    // 先推两条 stdout。
    assert!(reg.push_output(&id, SessionStream::Stdout, "a1\n"));
    assert!(reg.push_output(&id, SessionStream::Stdout, "a2\n"));
    assert!(reg.push_output(&id, SessionStream::Stderr, "e1\n"));
    let r1 = match reg.poll_output(&id, None, SystemTime::now()) {
        OutputPollOutcome::Found { status, log_read, eof, .. } => {
            assert_eq!(status, JobStatus::Queued);
            assert!(!eof, "非终态恒 eof=false");
            log_read
        }
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(!r1.truncated);
    assert_eq!(r1.items.len(), 3);
    let seqs: Vec<u64> = r1.items.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3]);
    // 续读不重不漏。
    let r2 = match reg.poll_output(&id, Some(r1.next_cursor), SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(r2.items.is_empty(), "无新数据不应重复返回: {:?}", r2.items);
    // 推送更晚数据后按游标只取增量。
    assert!(reg.push_output(&id, SessionStream::Stdout, "b3\n"));
    let r3 = match reg.poll_output(&id, Some(r1.next_cursor), SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(!r3.truncated);
    assert_eq!(r3.items.len(), 1);
    assert_eq!(r3.items[0].seq, 4);
    assert_eq!(r3.items[0].text, "b3\n");
}

#[test]
fn poll_output_eof_after_terminal_and_expired() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg);
    // 终态无输出 → eof=true、items 空。
    assert!(reg.complete(&id, ok_outcome(JobStatus::Succeeded), false));
    match reg.poll_output(&id, None, SystemTime::now()) {
        OutputPollOutcome::Found { status, log_read, eof, .. } => {
            assert_eq!(status, JobStatus::Succeeded);
            assert!(log_read.items.is_empty());
            assert!(eof, "终态且无输出应 eof=true");
        }
        other => panic!("expected Found, got {other:?}"),
    }
    // 过期 → Expired（输出缓冲随记录一并清理）。
    let far = SystemTime::now() + Duration::from_secs(200);
    assert!(matches!(reg.poll_output(&id, None, far), OutputPollOutcome::Expired));
    assert_eq!(reg.stats().total, 0);
    assert_eq!(reg.stats().output_retained_bytes, 0);
}

#[test]
fn ring_cap_drops_oldest_and_cursor_reports_truncated() {
    // 故意把缓冲压到 64 B：推 20 条 10 B 文本 → 只保留最近 ~6 条。
    let reg = ToolJobRegistry::new(JobLimits {
        output_buffer_bytes: 64,
        ..limits()
    });
    let id = register_default(&reg);
    for i in 1..=20u32 {
        reg.push_output(&id, SessionStream::Stdout, &format!("x{i:02}:abcdef\n"));
    }
    let r = match reg.poll_output(&id, None, SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(!r.truncated, "从头轮询不标 truncated");
    assert!(r.items.len() < 20, "环形应丢弃最旧: {:?}", r.items.len());
    let first_seq = r.items[0].seq;
    assert!(first_seq > 1, "最早的 seq 已被环形丢弃: {first_seq}");
    // 落后游标（早于最早保留）→ truncated=true 且从最早可用重放。
    let behind = match reg.poll_output(&id, Some(first_seq - 1), SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(behind.truncated, "落后游标应标 truncated");
    assert_eq!(behind.items[0].seq, first_seq);
    // cursor=0 视为"从头"，不标 truncated。
    let zero = match reg.poll_output(&id, Some(0), SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(!zero.truncated, "cursor=0 应从最早可用起且不标 truncated");
    assert_eq!(zero.items[0].seq, first_seq);
}

#[test]
fn terminal_trim_keeps_per_stream_tail_and_outputs_removed_on_cleanup() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg); // spawn.max_output_len = 1024
    // stdout 200 条 × 20 B = 4000 B；stderr 少一些。
    for i in 1..=200u32 {
        reg.push_output(&id, SessionStream::Stdout, &format!("y{i:03}:0123456789abcdef\n"));
    }
    for i in 1..=50u32 {
        reg.push_output(&id, SessionStream::Stderr, &format!("err{i:03}:12345\n"));
    }
    assert!(reg.complete(&id, ok_outcome(JobStatus::Succeeded), false));
    let r = match reg.poll_output(&id, None, SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    assert!(!r.items.is_empty(), "终态尾部应仍可读");
    let stdout_text: String = r
        .items
        .iter()
        .filter(|e| e.stream == SessionStream::Stdout)
        .map(|e| e.text.as_str())
        .collect();
    let stderr_text: String = r
        .items
        .iter()
        .filter(|e| e.stream == SessionStream::Stderr)
        .map(|e| e.text.as_str())
        .collect();
    // 终态裁剪：各流尾部 ≤ max_output_len(1024)，不拆元素（允许单条越界）。
    assert!(
        stdout_text.len() <= 1024 + 20,
        "stdout 尾部应 ≤ 1024：{}",
        stdout_text.len()
    );
    assert!(
        stderr_text.len() <= 1024 + 20,
        "stderr 尾部应 ≤ 1024：{}",
        stderr_text.len()
    );
    // 容量清理后输出侧表一并删除。
    let far = SystemTime::now() + Duration::from_secs(200);
    assert_eq!(reg.cleanup(far), 1);
    assert!(matches!(reg.poll_output(&id, None, far), OutputPollOutcome::Expired));
    assert_eq!(reg.stats().output_retained_bytes, 0);
}

/// 终态裁剪的**内存优先**边界：末尾单条超大元素（无法拆到 ≤ cap）且排在其后的已达标流
/// 已被整体丢弃——锁定"仅剩 1 条兜底、内存仍受限"的实际语义（注释/契约明示尽力）。
#[test]
fn terminal_trim_memory_first_when_trailing_single_over_cap() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(100),
        grace: Duration::from_secs(10),
        ..limits()
    });
    let id = register_default(&reg); // spawn.max_output_len = 1024
    // stdout 50 条 × 20 B = 1000 B（达标）；末尾单条 stderr 3000 B > cap(1024) 无法拆。
    for i in 1..=50u32 {
        reg.push_output(&id, SessionStream::Stdout, &format!("s{i:03}:0123456789abcdef\n"));
    }
    reg.push_output(
        &id,
        SessionStream::Stderr,
        &"E".repeat(3000),
    );
    assert!(reg.complete(&id, ok_outcome(JobStatus::Succeeded), false));
    let r = match reg.poll_output(&id, None, SystemTime::now()) {
        OutputPollOutcome::Found { log_read, .. } => log_read,
        other => panic!("expected Found, got {other:?}"),
    };
    // 兜底：至少 1 条可读（此处只剩无法拆掉的单条 stderr），且整体字节受限。
    assert!(!r.items.is_empty());
    let stderr_text: String = r
        .items
        .iter()
        .filter(|e| e.stream == SessionStream::Stderr)
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(stderr_text.len(), 3000, "单条不拆：完整保留");
    assert!(
        reg.stats().output_retained_bytes <= 3000,
        "整体保留 ≤ 单条超大元素"
    );
}
