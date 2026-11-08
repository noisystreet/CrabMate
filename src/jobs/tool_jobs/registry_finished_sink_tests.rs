use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
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
        None,
    )
    .expect("register")
}

fn succeeded() -> JobOutcome {
    JobOutcome {
        status: JobStatus::Succeeded,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    }
}

#[test]
fn finished_sink_invoked_once_on_complete() {
    let reg = ToolJobRegistry::new(limits());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen_status = Arc::new(Mutex::new(None::<JobStatus>));
    let sink: JobFinishedSink = {
        let calls = Arc::clone(&calls);
        let seen_status = Arc::clone(&seen_status);
        Arc::new(move |id: &str, outcome: &JobOutcome| {
            assert!(id.starts_with("tooljob_"), "id: {id}");
            *seen_status.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome.status);
            calls.fetch_add(1, Ordering::SeqCst);
        })
    };
    let id = reg
        .register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            Some(sink),
        )
        .expect("register");
    reg.try_start().expect("start");
    let ok = succeeded();
    assert!(reg.complete(&id, ok.clone(), false));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "终态补发应恰好一次");
    assert_eq!(
        *seen_status.lock().unwrap_or_else(|e| e.into_inner()),
        Some(JobStatus::Succeeded)
    );
    // 终态不可覆盖：再次 complete 不触发补发。
    assert!(!reg.complete(&id, ok, false));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "重复 complete 不得再次补发");
}

#[test]
fn finished_sink_absent_is_noop() {
    // 无 sink（运维 CLI / 非 Web 路径）：complete 正常返回，不 panic。
    let reg = ToolJobRegistry::new(limits());
    let id = register_default(&reg);
    reg.try_start().expect("start");
    assert!(reg.complete(&id, succeeded(), false));
}

#[test]
fn finished_sink_dropped_on_ttl_cleanup_without_invocation() {
    let reg = ToolJobRegistry::new(JobLimits {
        ttl: Duration::from_secs(1),
        grace: Duration::from_secs(1),
        ..limits()
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let sink: JobFinishedSink = {
        let calls = Arc::clone(&calls);
        Arc::new(move |_id: &str, _o: &JobOutcome| {
            calls.fetch_add(1, Ordering::SeqCst);
        })
    };
    let id = reg
        .register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            Some(sink),
        )
        .expect("register");
    reg.try_start().expect("start");
    assert!(reg.complete(&id, succeeded(), false));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // TTL 清理后侧表条目应被移除（不泄漏）。
    let removed = reg.cleanup(SystemTime::now() + Duration::from_secs(10));
    assert_eq!(removed, 1);
    assert!(reg.get(&id).is_none());
}

#[test]
fn finished_sink_invoked_on_cancel_queued() {
    let reg = ToolJobRegistry::new(limits());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen_status = Arc::new(Mutex::new(None::<JobStatus>));
    let seen_code = Arc::new(Mutex::new(None::<String>));
    let sink: JobFinishedSink = {
        let calls = Arc::clone(&calls);
        let seen_status = Arc::clone(&seen_status);
        let seen_code = Arc::clone(&seen_code);
        Arc::new(move |_id: &str, outcome: &JobOutcome| {
            *seen_status.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome.status);
            *seen_code.lock().unwrap_or_else(|e| e.into_inner()) = outcome.error_code.clone();
            calls.fetch_add(1, Ordering::SeqCst);
        })
    };
    // 占满并发位，使第二个任务停在 `queued`。
    let _running = reg
        .register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            None,
        )
        .expect("running");
    reg.try_start().expect("start");
    let queued = reg
        .register(
            PathBuf::from("/w"),
            None,
            spawn_default(),
            r#"{"command":"true"}"#.to_string(),
            Some(sink),
        )
        .expect("queued");
    assert_eq!(reg.get(&queued).expect("rec").status, JobStatus::Queued);
    assert_eq!(reg.cancel(&queued), CancelOutcome::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "queued 取消应补发一次");
    assert_eq!(
        *seen_status.lock().unwrap_or_else(|e| e.into_inner()),
        Some(JobStatus::Cancelled)
    );
    assert_eq!(
        seen_code
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref(),
        Some("cancelled")
    );
    // 幂等：再次 cancel 命中 AlreadyFinished，不重复补发。
    assert_eq!(
        reg.cancel(&queued),
        CancelOutcome::AlreadyFinished(JobStatus::Cancelled)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "重复 cancel 不得再次补发");
}
