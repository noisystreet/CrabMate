use super::*;
use std::io::{self, Read};
use std::process::Command;

struct BlockForever;

impl Read for BlockForever {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        thread::park();
        Ok(0)
    }
}

#[cfg(unix)]
#[test]
fn wait_session_timeout_kills_bash_sleep_grandchild() {
    let marker = format!("cm_p0_sleep_{}", std::process::id());
    let mut cmd = Command::new("bash");
    cmd.args(["-c", &format!("sleep 60 # {marker}")]);
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn bash");
    let pid = child.id();
    let r = wait_child_session(child, &SubprocessWaitCtl::with_wall_secs(1), 4096)
        .expect("wait");
    assert_eq!(r.kind, SessionStopKind::Timeout);
    assert!(r.killed);
    assert_eq!(r.child_pid, pid);
    thread::sleep(Duration::from_millis(200));
    assert!(!unix_pid_alive(pid), "direct child still alive pid={pid}");
    assert!(
        !proc_cmdline_contains(&marker),
        "grandchild sleep still listed for {marker}"
    );
}

#[cfg(unix)]
#[test]
fn wait_session_cancel_stops_sleep() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cmd = Command::new("sleep");
    cmd.arg("60");
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn sleep");
    let pid = child.id();
    let ctl = SubprocessWaitCtl {
        wall: Some(Duration::from_secs(30)),
        cancel: Some(Arc::clone(&cancel)),
        extra_stop: None,
        chunk_sink: None,
        uncapped_live: false,
    };
    let handle = thread::spawn(move || wait_child_session(child, &ctl, 1024));
    thread::sleep(Duration::from_millis(150));
    cancel.store(true, Ordering::SeqCst);
    let r = handle.join().expect("join").expect("wait");
    assert_eq!(r.kind, SessionStopKind::Cancelled);
    thread::sleep(Duration::from_millis(200));
    assert!(!unix_pid_alive(pid));
}

#[cfg(unix)]
#[test]
fn wait_session_stderr_flood_does_not_deadlock_before_timeout() {
    let mut cmd = Command::new("bash");
    cmd.args(["-c", "while true; do printf 'x%.0s' {1..1000} >&2; done"]);
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn flood");
    let r = wait_child_session(child, &SubprocessWaitCtl::with_wall_secs(1), 2048)
        .expect("wait");
    assert_eq!(r.kind, SessionStopKind::Timeout);
    assert!(!r.stderr.is_empty());
    assert!(r.stderr.len() <= 2048);
}

#[test]
fn wait_session_echo_exits_cleanly() {
    let mut cmd = Command::new("echo");
    cmd.arg("p0-ok");
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn echo");
    let r = wait_child_session(child, &SubprocessWaitCtl::default(), 4096).expect("wait");
    assert_eq!(r.kind, SessionStopKind::Exited);
    assert!(!r.killed);
    let stdout = String::from_utf8_lossy(&r.stdout);
    assert!(stdout.contains("p0-ok"), "{stdout:?}");
}

#[test]
fn take_drain_returns_before_blocked_pipe_eof() {
    let buf = spawn_drain(Some(BlockForever), 64, SessionStream::Stdout, None, false);
    let t0 = Instant::now();
    let v = take_drain(buf, Duration::from_millis(150));
    assert!(
        t0.elapsed() < Duration::from_millis(800),
        "drain join must be bounded, elapsed={:?}",
        t0.elapsed()
    );
    assert!(v.is_empty());
}

#[cfg(unix)]
#[test]
fn wait_session_chunk_sink_emits_stdout_and_stderr_deltas() {
    let chunks = Arc::new(Mutex::new(Vec::<(SessionStream, String)>::new()));
    let chunks_cb = Arc::clone(&chunks);
    let mut cmd = Command::new("bash");
    cmd.args(["-c", "echo p1-chunk-a; echo p1-err-b >&2; echo p1-chunk-c"]);
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn");
    let ctl = SubprocessWaitCtl {
        wall: Some(Duration::from_secs(5)),
        cancel: None,
        extra_stop: None,
        chunk_sink: Some(Arc::new(move |stream, bytes| {
            chunks_cb.lock().expect("lock").push((
                stream,
                String::from_utf8_lossy(bytes).into_owned(),
            ));
            true
        })),
        uncapped_live: false,
    };
    let r = wait_child_session(child, &ctl, 4096).expect("wait");
    assert_eq!(r.kind, SessionStopKind::Exited);
    let events = chunks.lock().expect("lock").clone();
    assert!(!events.is_empty(), "expected live chunks");
    let stdout: String = events
        .iter()
        .filter(|(s, _)| *s == SessionStream::Stdout)
        .map(|(_, t)| t.as_str())
        .collect();
    let stderr: String = events
        .iter()
        .filter(|(s, _)| *s == SessionStream::Stderr)
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(stdout.contains("p1-chunk-a"), "{stdout:?}");
    assert!(stdout.contains("p1-chunk-c"), "{stdout:?}");
    assert!(stderr.contains("p1-err-b"), "{stderr:?}");
    assert_eq!(stdout.as_bytes(), r.stdout.as_slice());
    assert_eq!(stderr.as_bytes(), r.stderr.as_slice());
}

#[cfg(unix)]
#[test]
fn wait_session_chunk_sink_seq_pieces_are_monotonic_concat() {
    let pieces = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let pieces_cb = Arc::clone(&pieces);
    let mut cmd = Command::new("bash");
    cmd.args(["-c", "echo first; sleep 0.25; echo second"]);
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn");
    let ctl = SubprocessWaitCtl {
        wall: Some(Duration::from_secs(5)),
        cancel: None,
        extra_stop: None,
        chunk_sink: Some(Arc::new(move |stream, bytes| {
            if stream == SessionStream::Stdout && !bytes.is_empty() {
                pieces_cb.lock().expect("lock").push(bytes.to_vec());
            }
            true
        })),
        uncapped_live: false,
    };
    let r = wait_child_session(child, &ctl, 4096).expect("wait");
    let parts = pieces.lock().expect("lock").clone();
    assert!(
        parts.len() >= 2,
        "expected at least two stdout deltas, got {parts:?}"
    );
    let joined: Vec<u8> = parts.into_iter().flatten().collect();
    assert_eq!(joined, r.stdout);
}

#[test]
fn take_utf8_text_holds_incomplete_then_completes() {
    let mut pending = Vec::new();
    assert_eq!(take_utf8_text(&mut pending, &[0xe4, 0xb8], false), "");
    assert_eq!(pending, vec![0xe4, 0xb8]);
    assert_eq!(take_utf8_text(&mut pending, &[0xad], false), "中");
    assert!(pending.is_empty());
}

#[test]
fn take_utf8_text_finish_replaces_incomplete() {
    let mut pending = Vec::new();
    assert_eq!(take_utf8_text(&mut pending, &[0xe4], true), "\u{FFFD}");
    assert!(pending.is_empty());
}

#[cfg(unix)]
#[test]
fn run_and_capture_timeout_kills_process_group_with_partial_output() {
    // 覆盖 `run_and_format*` 类工具迁移后共用的 `run_and_capture`：
    // 超时对进程组 SIGTERM→SIGKILL，且已捕获输出随结果返回（孙进程同样被清）。
    let marker = format!("cm_capture_sleep_{}", std::process::id());
    let mut cmd = Command::new("bash");
    cmd.args([
        "-c",
        &format!("echo partial-out; sleep 60 # {marker}"),
    ]);
    let r = run_and_capture(cmd, 4096, Some(1)).expect("run");
    assert_eq!(r.kind, SessionStopKind::Timeout);
    assert!(r.killed);
    assert!(
        String::from_utf8_lossy(&r.stdout).contains("partial-out"),
        "{:?}",
        r.stdout
    );
    thread::sleep(Duration::from_millis(200));
    assert!(
        !proc_cmdline_contains(&marker),
        "grandchild sleep still listed for {marker}"
    );
}

#[test]
fn run_and_capture_no_wall_exits_cleanly() {
    let mut cmd = Command::new("echo");
    cmd.arg("capture-ok");
    let r = run_and_capture(cmd, 4096, None).expect("run");
    assert_eq!(r.kind, SessionStopKind::Exited);
    assert!(!r.killed);
    assert!(String::from_utf8_lossy(&r.stdout).contains("capture-ok"));
}

#[cfg(unix)]
#[test]
fn wait_session_chunk_sink_false_retries_same_bytes() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fails_left = Arc::new(AtomicUsize::new(2));
    let got = Arc::new(Mutex::new(Vec::<u8>::new()));
    let fails_cb = Arc::clone(&fails_left);
    let got_cb = Arc::clone(&got);
    let mut cmd = Command::new("echo");
    cmd.arg("retry-me");
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn");
    let ctl = SubprocessWaitCtl {
        wall: Some(Duration::from_secs(5)),
        cancel: None,
        extra_stop: None,
        chunk_sink: Some(Arc::new(move |stream, bytes| {
            if bytes.is_empty() {
                return true;
            }
            if stream != SessionStream::Stdout {
                return true;
            }
            if fails_cb.load(Ordering::SeqCst) > 0 {
                fails_cb.fetch_sub(1, Ordering::SeqCst);
                return false;
            }
            got_cb.lock().expect("lock").extend_from_slice(bytes);
            true
        })),
        uncapped_live: false,
    };
    let r = wait_child_session(child, &ctl, 4096).expect("wait");
    let live = got.lock().expect("lock").clone();
    assert_eq!(live, r.stdout);
    assert!(String::from_utf8_lossy(&live).contains("retry-me"));
}

/// 实时流截断语义：默认（`uncapped_live=false`）sink 只收到 kept 前缀（≤ cap）；
/// `uncapped_live=true` 时 sink 收到**全量**输出（kept 快照仍前缀截断）。
#[cfg(unix)]
#[test]
fn uncapped_live_streams_beyond_capture_cap_but_kept_snapshot_stays_capped() {
    let run = |uncapped: bool| -> (Vec<u8>, Vec<u8>) {
        let stdout_live = Arc::new(Mutex::new(Vec::<u8>::new()));
        let live_cb = Arc::clone(&stdout_live);
        let mut cmd = Command::new("bash");
        cmd.args(["-c", "awk 'BEGIN{for(i=0;i<6000;i++)printf \"a\"}'"]);
        prepare_piped_process_group(&mut cmd);
        let child = cmd.spawn().expect("spawn");
        let ctl = SubprocessWaitCtl {
            wall: Some(Duration::from_secs(5)),
            cancel: None,
            extra_stop: None,
            chunk_sink: Some(Arc::new(move |stream, bytes| {
                if stream == SessionStream::Stdout && !bytes.is_empty() {
                    live_cb.lock().expect("lock").extend_from_slice(bytes);
                }
                true
            })),
            uncapped_live: uncapped,
        };
        let r = wait_child_session(child, &ctl, 2048).expect("wait");
        let live = stdout_live.lock().expect("lock").clone();
        (live, r.stdout)
    };

    // 默认：sink 只收到 kept 前缀增量（满 cap 后不再投递）——既有语义回归。
    let (live_default, kept_default) = run(false);
    assert_eq!(live_default, kept_default, "默认 sink 应与 kept 快照一致");
    assert!(live_default.len() <= 2048, "默认应被 cap 截断：{}", live_default.len());

    // uncapped：sink 收到全量（> cap），kept 快照仍 ≤ cap。
    let (live_uncapped, kept_uncapped) = run(true);
    assert!(
        live_uncapped.len() >= 6000,
        "uncapped 实时流应全量可达：{}",
        live_uncapped.len()
    );
    assert!(
        kept_uncapped.len() <= 2048,
        "kept 快照仍按 cap 前缀截断：{}",
        kept_uncapped.len()
    );
    assert!(live_uncapped.len() > kept_uncapped.len());
}

#[cfg(unix)]
#[test]
fn session_stats_record_timeout_killed_and_live_returns_to_zero() {
    let before = session_stats_snapshot();
    let mut cmd = Command::new("sleep");
    cmd.arg("60");
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn");
    let r = wait_child_session(child, &SubprocessWaitCtl::with_wall_secs(1), 1024)
        .expect("wait");
    assert_eq!(r.kind, SessionStopKind::Timeout);
    let after = session_stats_snapshot();
    // 其它测试并发跑会话，只用单调下界断言。
    assert!(after.spawns > before.spawns, "{after:?}");
    assert!(after.completed > before.completed, "{after:?}");
    assert!(after.timeouts > before.timeouts, "{after:?}");
    assert!(after.killed > before.killed, "{after:?}");
    assert!(after.live >= 0, "{after:?}");
    assert!(
        after.duration_mean_ms > 0,
        "本会话至少 wall 1s + reap 时间，均值不应为 0: {after:?}"
    );
    // 直方图测试会并发注入时长（非会话），此处只断言本会话带来至少一次累加。
    assert!(
        after.duration_buckets.iter().sum::<u64>() > before.duration_buckets.iter().sum::<u64>(),
        "{after:?}"
    );
}

#[test]
fn session_stats_histogram_places_duration_into_overflow_bucket() {
    // 直接驱动内部计数：超大时长应落入最后一桶（>600s 溢出），小时长落入首桶。
    let before = session_stats_snapshot();
    record_session_duration(42);
    record_session_duration(900_000);
    let after = session_stats_snapshot();
    assert!(after.duration_buckets[0] > before.duration_buckets[0], "{after:?}");
    assert!(
        after.duration_buckets[SESSION_DURATION_BUCKET_MS.len() - 1]
            > before.duration_buckets[SESSION_DURATION_BUCKET_MS.len() - 1],
        "{after:?}"
    );
    assert!(
        after.duration_mean_ms > before.duration_mean_ms,
        "累计均值应随两段新增时长上升: {after:?}"
    );
}
