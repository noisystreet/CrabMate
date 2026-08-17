//! 可取消的子进程会话：进程组、并发排空管道、墙钟超时。
//!
//! 当前实现走 **`std::process` + 阻塞轮询**（`run_command` 仍在 `spawn_blocking` 内调用）。
//! 长命令仍占用阻塞线程；迁 `tokio::process` 见 `docs/design/long_running_tool_execution_todo.md`。

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// SIGTERM 之后、SIGKILL 之前的宽限。
pub const TERM_GRACE: Duration = Duration::from_millis(400);
const POLL: Duration = Duration::from_millis(50);
/// 杀进程后等待退出的上限（不再 `child.wait()` 无界阻塞）。
const REAP_WAIT: Duration = Duration::from_secs(2);
/// 管道 drain 线程 join 上限；超时则带走已捕获字节，避免孤儿占管导致永久卡住。
const DRAIN_JOIN: Duration = Duration::from_secs(2);

/// 等待控制：墙钟、协作取消、额外停止条件（如 SSE `Sender::is_closed`）。
#[derive(Clone, Default)]
pub struct SubprocessWaitCtl {
    pub wall: Option<Duration>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub extra_stop: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl SubprocessWaitCtl {
    #[must_use]
    pub fn with_wall_secs(secs: u64) -> Self {
        Self {
            wall: Some(Duration::from_secs(secs.max(1))),
            ..Self::default()
        }
    }
}

/// 会话结束原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStopKind {
    Exited,
    Timeout,
    Cancelled,
}

/// 等待结果（stdout/stderr 已按 `max_capture_bytes` 截断；管道仍读到 EOF）。
#[derive(Debug)]
pub struct SessionWaitResult {
    pub kind: SessionStopKind,
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub killed: bool,
    pub child_pid: u32,
}

struct DrainBuf {
    done_rx: mpsc::Receiver<()>,
    kept: Arc<Mutex<Vec<u8>>>,
}

/// 配置 stdin 关闭、stdout/stderr 管道；Unix 上子进程成为**新进程组组长**。
pub fn prepare_piped_process_group(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}

/// `spawn` 后等待：超时/取消时对进程组 SIGTERM→SIGKILL（Unix）。
pub fn wait_child_session(
    mut child: Child,
    ctl: &SubprocessWaitCtl,
    max_capture_bytes: usize,
) -> io::Result<SessionWaitResult> {
    let child_pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = max_capture_bytes.max(1);
    let out_buf = spawn_drain(stdout, cap);
    let err_buf = spawn_drain(stderr, cap);
    let deadline = ctl.wall.map(|d| Instant::now() + d);

    let mut killed = false;
    let mut kind = SessionStopKind::Exited;
    let mut wait_err = None;
    let status = loop {
        if stop_requested(ctl) {
            kind = SessionStopKind::Cancelled;
            terminate_child_group(&mut child, child_pid, "cancel");
            killed = true;
            break reap_after_kill(&mut child);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            kind = SessionStopKind::Timeout;
            terminate_child_group(&mut child, child_pid, "timeout");
            killed = true;
            break reap_after_kill(&mut child);
        }
        match child.try_wait() {
            Ok(Some(st)) => break Ok(Some(st)),
            Ok(None) => thread::sleep(POLL),
            Err(e) => {
                wait_err = Some(e);
                terminate_child_group(&mut child, child_pid, "try_wait");
                killed = true;
                let _ = reap_after_kill(&mut child);
                break Ok(None);
            }
        }
    };

    let stdout = take_drain(out_buf, DRAIN_JOIN);
    let stderr = take_drain(err_buf, DRAIN_JOIN);
    if let Some(e) = wait_err {
        return Err(e);
    }
    Ok(SessionWaitResult {
        kind,
        status: status.ok().flatten(),
        stdout,
        stderr,
        killed,
        child_pid,
    })
}

fn stop_requested(ctl: &SubprocessWaitCtl) -> bool {
    if ctl
        .cancel
        .as_ref()
        .is_some_and(|c| c.load(Ordering::SeqCst))
    {
        return true;
    }
    ctl.extra_stop.as_ref().is_some_and(|f| f())
}

fn spawn_drain<R: Read + Send + 'static>(pipe: Option<R>, max_bytes: usize) -> DrainBuf {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let kept_th = Arc::clone(&kept);
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Some(mut r) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match r.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => append_captured(&kept_th, &chunk[..n], max_bytes),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        let _ = done_tx.send(());
    });
    DrainBuf { done_rx, kept }
}

fn append_captured(kept: &Mutex<Vec<u8>>, chunk: &[u8], max_bytes: usize) {
    let Ok(mut g) = kept.lock() else {
        return;
    };
    if g.len() >= max_bytes {
        return;
    }
    let room = max_bytes - g.len();
    g.extend_from_slice(&chunk[..chunk.len().min(room)]);
}

fn snapshot_kept(kept: &Mutex<Vec<u8>>) -> Vec<u8> {
    kept.lock().map(|g| g.clone()).unwrap_or_default()
}

fn take_drain(buf: DrainBuf, limit: Duration) -> Vec<u8> {
    let _ = buf.done_rx.recv_timeout(limit);
    snapshot_kept(&buf.kept)
}

fn terminate_child_group(child: &mut Child, pid: u32, reason: &'static str) {
    log::warn!(
        target: "crabmate",
        "subprocess session kill pid={} reason={}",
        pid,
        reason
    );
    #[cfg(unix)]
    unix_term_then_kill(pid);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        thread::sleep(TERM_GRACE);
    }
    let _ = child.kill();
}

fn reap_after_kill(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    let until = Instant::now() + REAP_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Ok(Some(st)),
            Ok(None) if Instant::now() < until => thread::sleep(POLL),
            Ok(None) => {
                let _ = child.kill();
                let hard = Instant::now() + POLL.saturating_mul(4);
                while Instant::now() < hard {
                    if let Ok(Some(st)) = child.try_wait() {
                        return Ok(Some(st));
                    }
                    thread::sleep(POLL);
                }
                return Ok(None);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
fn unix_term_then_kill(pid: u32) {
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    let child = Pid::from_raw(pid as i32);
    let self_pg = nix::unistd::getpgrp();
    if child == self_pg {
        let _ = signal::kill(child, Signal::SIGTERM);
        thread::sleep(TERM_GRACE);
        let _ = signal::kill(child, Signal::SIGKILL);
        return;
    }
    let _ = signal::killpg(child, Signal::SIGTERM);
    thread::sleep(TERM_GRACE);
    let _ = signal::killpg(child, Signal::SIGKILL);
    let _ = signal::kill(child, Signal::SIGKILL);
}

/// `kill(pid, 0)`：进程是否仍存在（测试与 reap 断言）。
#[must_use]
pub fn unix_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal;
        use nix::unistd::Pid;
        signal::kill(Pid::from_raw(pid as i32), None::<nix::sys::signal::Signal>).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// `/proc/*/cmdline` 是否含标记（测试用，不依赖 `pgrep`）。
#[cfg(all(test, unix))]
pub(crate) fn proc_cmdline_contains(marker: &str) -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return false;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(s) = name.to_str() else {
            continue;
        };
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(raw) = std::fs::read(ent.path().join("cmdline")) else {
            continue;
        };
        let text: String = raw
            .iter()
            .map(|&b| if b == 0 { ' ' } else { char::from(b) })
            .collect();
        if text.contains(marker) {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "subprocess_session_tests.rs"]
mod tests;
