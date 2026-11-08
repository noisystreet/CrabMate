use super::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

// 独立子模块（registry.rs 顶层 `#[path]` include）：`background_job_wait` 的事件唤醒语义。
//
// 这些用例要钉住的不是「等到了」，而是「**怎么**等到的」：
// - 事件唤醒而非定长轮询（断言耗时远小于墙钟上限）→ 若有人把 Condvar 改回短睡轮询，
//   `*_wakes_*` 系列会在 timeout 附近失败；
// - 超时不改状态（Q2 保持纯只读）；
// - 同 id 连续两次等待都能拿到**新**进展（同轮去重缓存的回归防线）。

fn limits() -> JobLimits {
    JobLimits {
        max_concurrent: 2,
        max_queued: 2,
        ttl: Duration::from_secs(3600),
        grace: Duration::from_secs(60),
        max_entries: 8,
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

fn register(reg: &ToolJobRegistry) -> String {
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
        stdout: b"done\n".to_vec(),
        stderr: Vec::new(),
        error_code: None,
        failure_category: None,
    }
}

#[test]
fn terminal_job_returns_immediately() {
    let reg = ToolJobRegistry::new(limits());
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");
    assert!(reg.complete(&id, succeeded(), false), "complete");

    let started = Instant::now();
    let out = reg.wait_for(&id, None, Duration::from_secs(30));
    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Succeeded,
            matched: false
        }
    );
    // 已终态必须零等待返回（事件唤醒路径的第一步检查）。
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
}

#[test]
fn output_matches_wakes_on_push_without_waiting_for_full_timeout() {
    let reg = Arc::new(ToolJobRegistry::new(limits()));
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");

    let pusher = {
        let reg = Arc::clone(&reg);
        let id = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            reg.push_output(&id, SessionStream::Stdout, "dev server listening on 3000\n");
        })
    };

    let started = Instant::now();
    let out = reg.wait_for(&id, Some("listening on"), Duration::from_secs(30));
    pusher.join().expect("pusher");

    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Running,
            matched: true
        }
    );
    // 命中即返回：远早于 30s 上限 —— 若退回定长轮询/只在终态唤醒，这里会等满 30s。
    // 留 10s 余量而不是 5s：仍能区分「事件唤醒」与「等满超时」，又不容易被 CI 调度抖动误伤。
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn pattern_spanning_two_pushes_is_matched() {
    let reg = ToolRegistryGuard::new();
    let id = reg.register_job();
    reg.start();
    reg.push(&id, "listen");
    reg.push(&id, "ing on 3000");

    let out = reg
        .registry
        .wait_for(&id, Some("listening on"), Duration::from_secs(5));
    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Running,
            matched: true
        }
    );
}

/// 多字节（CJK）pattern 跨两次 push：验证接缝扫描在字符边界处不 panic 且仍能命中。
#[test]
fn multibyte_pattern_spanning_pushes_is_matched() {
    let reg = ToolRegistryGuard::new();
    let id = reg.register_job();
    reg.start();
    reg.push(&id, "正在启");
    reg.push(&id, "动服务器");

    let out = reg
        .registry
        .wait_for(&id, Some("正在启动服务器"), Duration::from_secs(5));
    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Running,
            matched: true
        }
    );
}

/// 命中优先于「正在取消」：日志里已有目标子串时，不应被告知「已请求取消」。
#[test]
fn pattern_hit_wins_over_pending_cancel() {
    let reg = ToolJobRegistry::new(limits());
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");
    reg.push_output(&id, SessionStream::Stdout, "listening on 3000\n");
    assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);

    assert_eq!(
        reg.wait_for(&id, Some("listening on"), Duration::from_secs(5)),
        WaitOutcome::Hit {
            status: JobStatus::Running,
            matched: true
        }
    );
}

#[test]
fn timeout_keeps_job_running_and_unchanged() {
    let reg = ToolJobRegistry::new(limits());
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");

    let out = reg.wait_for(&id, None, Duration::from_millis(120));
    assert_eq!(
        out,
        WaitOutcome::DeadlineReached {
            status: JobStatus::Running
        }
    );
    // Q2：超时不取消、不改状态。
    let rec = reg.get(&id).expect("record 仍在");
    assert_eq!(rec.status, JobStatus::Running);
    assert!(!rec.cancel_requested, "等待超时不得置取消标记");
}

#[test]
fn queued_job_is_woken_through_running_to_terminal() {
    let reg = Arc::new(ToolJobRegistry::new(limits()));
    let id = register(&reg);

    let waiter = {
        let reg = Arc::clone(&reg);
        let id = id.clone();
        std::thread::spawn(move || reg.wait_for(&id, None, Duration::from_secs(30)))
    };
    // 让等待者先进入 wait_timeout，再推进状态（覆盖「先等后变」的唤醒路径）。
    std::thread::sleep(Duration::from_millis(50));
    assert!(reg.try_start().is_some(), "try_start");
    assert!(reg.complete(&id, succeeded(), false), "complete");

    let out = waiter.join().expect("waiter");
    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Succeeded,
            matched: false
        }
    );
}

#[test]
fn cancel_queued_wakes_waiter_with_terminal_status() {
    let reg = Arc::new(ToolJobRegistry::new(limits()));
    let id = register(&reg);

    let waiter = {
        let reg = Arc::clone(&reg);
        let id = id.clone();
        std::thread::spawn(move || reg.wait_for(&id, None, Duration::from_secs(30)))
    };
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);

    let out = waiter.join().expect("waiter");
    assert_eq!(
        out,
        WaitOutcome::Hit {
            status: JobStatus::Cancelled,
            matched: false
        }
    );
}

/// `running` 中被取消：等待者**立即**返回 `CancelRequested`，不等 worker 收尾落地终态。
#[test]
fn cancel_running_releases_waiter_before_terminal() {
    let reg = Arc::new(ToolJobRegistry::new(limits()));
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");

    let waiter = {
        let reg = Arc::clone(&reg);
        let id = id.clone();
        std::thread::spawn(move || reg.wait_for(&id, None, Duration::from_secs(30)))
    };
    // 先让等待者进入 wait_timeout，再把取消标记置位（覆盖「先等后取消」）。
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(reg.cancel(&id), CancelOutcome::Cancelled);

    let started = Instant::now();
    let out = waiter.join().expect("waiter");
    assert_eq!(
        out,
        WaitOutcome::CancelRequested {
            status: JobStatus::Running
        },
        "取消标记置位后应立刻返回，而不是空等到终态/超时"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    // 取消不改变任务记录本身的状态：终态由 worker 收尾时写入（契约 §3.2）。
    let rec = reg.get(&id).expect("record 仍在");
    assert_eq!(rec.status, JobStatus::Running);
    assert!(rec.cancel_requested);
}

/// 同 id 连续两次等待都能拿到**新**进展：先等 `output_matches` 命中（任务仍在跑），
/// 再等终态。这是 `builtin_dedup_cache_exempt_tools` 必须豁免本工具的理由。
#[test]
fn consecutive_waits_same_id_see_new_progress() {
    let reg = ToolJobRegistry::new(limits());
    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");
    reg.push_output(&id, SessionStream::Stdout, "listening on 3000\n");

    let first = reg.wait_for(&id, Some("listening on"), Duration::from_secs(5));
    assert_eq!(
        first,
        WaitOutcome::Hit {
            status: JobStatus::Running,
            matched: true
        }
    );

    assert!(reg.complete(&id, succeeded(), false), "complete");
    let second = reg.wait_for(&id, Some("listening on"), Duration::from_secs(5));
    assert_eq!(
        second,
        WaitOutcome::Hit {
            status: JobStatus::Succeeded,
            matched: false
        },
        "第二次等待必须看到终态，不能被第一次的结果吃掉"
    );
}

#[test]
fn missing_and_expired_ids_are_reported() {
    let reg = ToolJobRegistry::new(limits());
    assert_eq!(
        reg.wait_for("tooljob_missing", None, Duration::from_millis(10)),
        WaitOutcome::NotFound
    );

    let id = register(&reg);
    assert!(reg.try_start().is_some(), "try_start");
    assert!(reg.complete(&id, succeeded(), false), "complete");
    assert!(reg.cleanup(SystemTime::now() + Duration::from_secs(3700)) >= 1);
    assert_eq!(
        reg.wait_for(&id, None, Duration::from_millis(10)),
        WaitOutcome::Expired
    );
}

/// 终态与 `wait_for` 的竞态：完成发生在等待者复查窗口内的任意时刻，都不能丢唤醒。
#[test]
fn repeated_races_between_complete_and_wait_never_hang() {
    for _ in 0..20 {
        let reg = Arc::new(ToolJobRegistry::new(limits()));
        let id = register(&reg);
        assert!(reg.try_start().is_some(), "try_start");

        let waiter = {
            let reg = Arc::clone(&reg);
            let id = id.clone();
            std::thread::spawn(move || reg.wait_for(&id, None, Duration::from_secs(10)))
        };
        assert!(reg.complete(&id, succeeded(), false), "complete");

        let out = waiter.join().expect("waiter");
        assert!(
            matches!(
                out,
                WaitOutcome::Hit {
                    status: JobStatus::Succeeded,
                    ..
                }
            ),
            "竞态下应看到终态而不是超时：{out:?}"
        );
    }
}

/// 小型夹具：避免每个用例重复 register/start/push。
struct ToolRegistryGuard {
    registry: Arc<ToolJobRegistry>,
}

impl ToolRegistryGuard {
    fn new() -> Self {
        Self {
            registry: Arc::new(ToolJobRegistry::new(limits())),
        }
    }

    fn register_job(&self) -> String {
        register(&self.registry)
    }

    fn start(&self) -> String {
        let rec = self.registry.try_start().expect("try_start");
        rec.id
    }

    fn push(&self, id: &str, text: &str) {
        assert!(
            self.registry.push_output(id, SessionStream::Stdout, text),
            "push_output"
        );
    }
}
