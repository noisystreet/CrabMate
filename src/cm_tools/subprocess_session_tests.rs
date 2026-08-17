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
    let buf = spawn_drain(Some(BlockForever), 64);
    let t0 = Instant::now();
    let v = take_drain(buf, Duration::from_millis(150));
    assert!(
        t0.elapsed() < Duration::from_millis(800),
        "drain join must be bounded, elapsed={:?}",
        t0.elapsed()
    );
    assert!(v.is_empty());
}
