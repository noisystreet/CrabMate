//! 可取消的子进程会话：进程组、并发排空管道、墙钟超时。
//!
//! 当前实现走 **`std::process` + 阻塞轮询**（`run_command` 仍在 `spawn_blocking` 内调用）。
//! 长命令仍占用阻塞线程；迁 `tokio::process` 见 `docs/design/long_running_tool_execution_todo.md`。
//!
//! 观测：进程内原子计数 + 时长直方图（见 [`session_stats_snapshot`]），供日志、测试与未来 metrics 端点读取。

use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
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

/// 会话时长直方图桶上界（毫秒）：`≤1s、≤5s、≤30s、≤120s、≤600s、>600s`（末桶 `u64::MAX` 为溢出）。
pub const SESSION_DURATION_BUCKET_MS: [u64; 6] = [1_000, 5_000, 30_000, 120_000, 600_000, u64::MAX];

/// 进程内会话观测快照（进程级累计；无外部指标后端时供日志/测试/未来 metrics 端点读取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStats {
    pub spawns: u64,
    /// 当前尚未结束的会话数（结束含正常退出与杀进程后 reap）。
    pub live: i64,
    pub completed: u64,
    pub timeouts: u64,
    pub cancelled: u64,
    /// 超时/取消/`try_wait` 失败时对进程组发信号的数量（与 `timeouts + cancelled` 近似，含 try_wait 异常）。
    pub killed: u64,
    /// 杀进程后 `REAP_WAIT` 内仍未确认退出的会话数（**残留子进程风险**，日志会带 pid）。
    pub reap_failed: u64,
    pub duration_mean_ms: u64,
    pub duration_buckets: [u64; SESSION_DURATION_BUCKET_MS.len()],
}

static STAT_SPAWNS: AtomicU64 = AtomicU64::new(0);
static STAT_LIVE: AtomicI64 = AtomicI64::new(0);
static STAT_COMPLETED: AtomicU64 = AtomicU64::new(0);
static STAT_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static STAT_CANCELLED: AtomicU64 = AtomicU64::new(0);
static STAT_KILLED: AtomicU64 = AtomicU64::new(0);
static STAT_REAP_FAILED: AtomicU64 = AtomicU64::new(0);
static STAT_DURATION_MS_SUM: AtomicU64 = AtomicU64::new(0);
static STAT_DURATION_COUNT: AtomicU64 = AtomicU64::new(0);
static STAT_DURATION_BUCKETS: [AtomicU64; SESSION_DURATION_BUCKET_MS.len()] =
    [const { AtomicU64::new(0) }; SESSION_DURATION_BUCKET_MS.len()];

/// 进程内会话观测快照（`duration_mean_ms` 为累计平均，`0` 表示尚无完成记录）。
#[must_use]
pub fn session_stats_snapshot() -> SessionStats {
    let count = STAT_DURATION_COUNT.load(Ordering::Relaxed);
    let sum = STAT_DURATION_MS_SUM.load(Ordering::Relaxed);
    let mut duration_buckets = [0u64; SESSION_DURATION_BUCKET_MS.len()];
    for (dst, src) in duration_buckets
        .iter_mut()
        .zip(STAT_DURATION_BUCKETS.iter())
    {
        *dst = src.load(Ordering::Relaxed);
    }
    SessionStats {
        spawns: STAT_SPAWNS.load(Ordering::Relaxed),
        live: STAT_LIVE.load(Ordering::Relaxed),
        completed: STAT_COMPLETED.load(Ordering::Relaxed),
        timeouts: STAT_TIMEOUTS.load(Ordering::Relaxed),
        cancelled: STAT_CANCELLED.load(Ordering::Relaxed),
        killed: STAT_KILLED.load(Ordering::Relaxed),
        reap_failed: STAT_REAP_FAILED.load(Ordering::Relaxed),
        duration_mean_ms: sum.checked_div(count).unwrap_or(0),
        duration_buckets,
    }
}

fn record_session_duration(duration_ms: u64) {
    STAT_DURATION_MS_SUM.fetch_add(duration_ms, Ordering::Relaxed);
    STAT_DURATION_COUNT.fetch_add(1, Ordering::Relaxed);
    for (bound, bucket) in SESSION_DURATION_BUCKET_MS
        .iter()
        .zip(STAT_DURATION_BUCKETS.iter())
    {
        if duration_ms <= *bound {
            bucket.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    // 超过所有上界（`>u64::MAX`，实际不可达）：落入末桶。
    STAT_DURATION_BUCKETS[SESSION_DURATION_BUCKET_MS.len() - 1].fetch_add(1, Ordering::Relaxed);
}

fn record_session_stats(
    child_pid: u32,
    kind: SessionStopKind,
    killed: bool,
    reap_failed: bool,
    duration_ms: u64,
) {
    record_session_duration(duration_ms);
    if killed {
        STAT_KILLED.fetch_add(1, Ordering::Relaxed);
    }
    match kind {
        SessionStopKind::Exited => {}
        SessionStopKind::Timeout => {
            STAT_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }
        SessionStopKind::Cancelled => {
            STAT_CANCELLED.fetch_add(1, Ordering::Relaxed);
        }
    }
    STAT_COMPLETED.fetch_add(1, Ordering::Relaxed);
    if reap_failed {
        STAT_REAP_FAILED.fetch_add(1, Ordering::Relaxed);
        log::warn!(
            target: "crabmate",
            "subprocess session reap 未确认 pid={} kind={:?} duration_ms={}（进程组可能残留）",
            child_pid,
            kind,
            duration_ms
        );
    }
    log::debug!(
        target: "crabmate",
        "subprocess session done pid={} kind={:?} killed={} duration_ms={}",
        child_pid,
        kind,
        killed,
        duration_ms
    );
}

/// 等待控制：墙钟、协作取消、额外停止条件（如 SSE `Sender::is_closed`）、可选增量回调。
#[derive(Clone, Default)]
pub struct SubprocessWaitCtl {
    pub wall: Option<Duration>,
    pub cancel: Option<Arc<AtomicBool>>,
    pub extra_stop: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// 已捕获字节的增量（不超过 `max_capture_bytes`）；宿主可转成 SSE `tool_output_chunk`。
    pub chunk_sink: Option<SessionChunkSink>,
}

/// stdout / stderr 管道增量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStream {
    Stdout,
    Stderr,
}

impl SessionStream {
    #[must_use]
    pub fn as_sse_label(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// 捕获缓冲新增字节；返回 `false` 时会话会把同一增量留下一轮再试（例如 SSE `try_send` 失败）。
pub type SessionChunkSink = Arc<dyn Fn(SessionStream, &[u8]) -> bool + Send + Sync>;

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

struct DrainEvent {
    stream: SessionStream,
    bytes: Vec<u8>,
}

type LiveChunks = Arc<Mutex<VecDeque<DrainEvent>>>;

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
    STAT_SPAWNS.fetch_add(1, Ordering::Relaxed);
    STAT_LIVE.fetch_add(1, Ordering::Relaxed);
    let t0 = Instant::now();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = max_capture_bytes.max(1);
    let live = ctl
        .chunk_sink
        .is_some()
        .then(|| Arc::new(Mutex::new(VecDeque::new())));
    let out_buf = spawn_drain(stdout, cap, SessionStream::Stdout, live.clone());
    let err_buf = spawn_drain(stderr, cap, SessionStream::Stderr, live.clone());
    let deadline = ctl.wall.map(|d| Instant::now() + d);

    let mut killed = false;
    let mut kind = SessionStopKind::Exited;
    let mut wait_err = None;
    let mut reap_failed = false;
    let status: io::Result<Option<ExitStatus>> = loop {
        flush_live_chunks(live.as_ref(), ctl.chunk_sink.as_ref());
        if stop_requested(ctl) {
            kind = SessionStopKind::Cancelled;
            terminate_child_group(&mut child, child_pid, "cancel");
            killed = true;
            reap_failed = reap_not_confirmed(reap_after_kill(&mut child));
            break Ok(None);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            kind = SessionStopKind::Timeout;
            terminate_child_group(&mut child, child_pid, "timeout");
            killed = true;
            reap_failed = reap_not_confirmed(reap_after_kill(&mut child));
            break Ok(None);
        }
        match child.try_wait() {
            Ok(Some(st)) => break Ok(Some(st)),
            Ok(None) => thread::sleep(POLL),
            Err(e) => {
                wait_err = Some(e);
                terminate_child_group(&mut child, child_pid, "try_wait");
                killed = true;
                reap_failed = reap_not_confirmed(reap_after_kill(&mut child));
                break Ok(None);
            }
        }
    };

    let stdout = take_drain(out_buf, DRAIN_JOIN);
    let stderr = take_drain(err_buf, DRAIN_JOIN);
    drain_live_chunks_after_reap(live.as_ref(), ctl.chunk_sink.as_ref());
    finish_chunk_sink(ctl.chunk_sink.as_ref());
    record_session_stats(
        child_pid,
        kind,
        killed,
        reap_failed,
        t0.elapsed().as_millis() as u64,
    );
    STAT_LIVE.fetch_sub(1, Ordering::Relaxed);
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

/// 一次性运行命令并等待：`spawn`（新进程组 + 并发排空管道）→ [`wait_child_session`]。
///
/// 供 `run_and_format*` 等测试/构建类工具复用，避免各工具各自抄一套 wait：
/// `wall_secs = None` 表示无墙钟（保持既有「靠外圈 `spawn_blocking` timeout」语义）；
/// `Some(secs)` 时超时对**进程组** SIGTERM→SIGKILL，已截断 stdout/stderr 仍随结果返回。
pub fn run_and_capture(
    mut cmd: Command,
    max_output_len: usize,
    wall_secs: Option<u64>,
) -> io::Result<SessionWaitResult> {
    prepare_piped_process_group(&mut cmd);
    let child = cmd.spawn()?;
    let ctl = SubprocessWaitCtl {
        wall: wall_secs.map(|s| Duration::from_secs(s.max(1))),
        ..SubprocessWaitCtl::default()
    };
    wait_child_session(child, &ctl, max_output_len)
}

/// 杀进程后是否在 `REAP_WAIT` 内**未**确认退出（残留风险）；`Err` 视同残留。
fn reap_not_confirmed(reap: io::Result<Option<ExitStatus>>) -> bool {
    reap.map(|r| r.is_none()).unwrap_or(true)
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

fn spawn_drain<R: Read + Send + 'static>(
    pipe: Option<R>,
    max_bytes: usize,
    stream: SessionStream,
    live: Option<LiveChunks>,
) -> DrainBuf {
    let kept = Arc::new(Mutex::new(Vec::new()));
    let kept_th = Arc::clone(&kept);
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Some(mut r) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match r.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => append_captured(&kept_th, &chunk[..n], max_bytes, stream, live.as_ref()),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        let _ = done_tx.send(());
    });
    DrainBuf { done_rx, kept }
}

fn append_captured(
    kept: &Mutex<Vec<u8>>,
    chunk: &[u8],
    max_bytes: usize,
    stream: SessionStream,
    live: Option<&LiveChunks>,
) {
    let added = {
        let Ok(mut g) = kept.lock() else {
            return;
        };
        if g.len() >= max_bytes {
            return;
        }
        let room = max_bytes - g.len();
        let n = chunk.len().min(room);
        g.extend_from_slice(&chunk[..n]);
        g[g.len() - n..].to_vec()
    };
    if added.is_empty() {
        return;
    }
    let Some(live) = live else {
        return;
    };
    if let Ok(mut q) = live.lock() {
        q.push_back(DrainEvent {
            stream,
            bytes: added,
        });
    }
}

fn snapshot_kept(kept: &Mutex<Vec<u8>>) -> Vec<u8> {
    kept.lock().map(|g| g.clone()).unwrap_or_default()
}

fn take_drain(buf: DrainBuf, limit: Duration) -> Vec<u8> {
    let _ = buf.done_rx.recv_timeout(limit);
    snapshot_kept(&buf.kept)
}

fn flush_live_chunks(live: Option<&LiveChunks>, sink: Option<&SessionChunkSink>) {
    let Some(sink) = sink else {
        return;
    };
    let Some(live) = live else {
        return;
    };
    loop {
        let ev = match live.lock() {
            Ok(mut q) => q.pop_front(),
            Err(_) => return,
        };
        let Some(ev) = ev else {
            break;
        };
        if ev.bytes.is_empty() {
            continue;
        }
        if !sink(ev.stream, &ev.bytes) {
            if let Ok(mut q) = live.lock() {
                q.push_front(ev);
            }
            break;
        }
    }
}

fn live_queue_empty(live: Option<&LiveChunks>) -> bool {
    live.and_then(|q| q.lock().ok())
        .is_none_or(|g| g.is_empty())
}

fn drain_live_chunks_after_reap(live: Option<&LiveChunks>, sink: Option<&SessionChunkSink>) {
    for _ in 0..64 {
        if live_queue_empty(live) {
            break;
        }
        flush_live_chunks(live, sink);
    }
}

fn finish_chunk_sink(sink: Option<&SessionChunkSink>) {
    let Some(sink) = sink else {
        return;
    };
    let _ = sink(SessionStream::Stdout, &[]);
    let _ = sink(SessionStream::Stderr, &[]);
}

/// 把 `incoming` 接到 `pending`，取出完整 UTF-8（非法字节变成 U+FFFD）。
/// `finish` 时把末尾不完整序列也换成 U+FFFD。
pub fn take_utf8_text(pending: &mut Vec<u8>, incoming: &[u8], finish: bool) -> String {
    pending.extend_from_slice(incoming);
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    let ok = std::str::from_utf8(&pending[..valid])
                        .expect("valid_up_to is UTF-8");
                    out.push_str(ok);
                    pending.drain(..valid);
                    continue;
                }
                if let Some(n) = e.error_len() {
                    out.push('\u{FFFD}');
                    let n = n.max(1).min(pending.len());
                    pending.drain(..n);
                    continue;
                }
                if finish && !pending.is_empty() {
                    out.push('\u{FFFD}');
                    pending.clear();
                }
                return out;
            }
        }
    }
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
