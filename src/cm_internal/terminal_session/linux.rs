//! Linux PTY **`terminal_session`**：`forkpty` + 会话表；输出经 SSE **`tool_output_chunk`** 增量下发。

use std::collections::HashMap;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, dup, read, tcgetpgrp, write};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use super::TerminalSseSink;
use super::pty_io::{
    child_gone_after_poll, fork_pty_session, reap_child_background, reap_child_blocking,
    resize_session_master,
};
use super::sse_chunk::{emit_tool_chunk, push_truncated};
use crate::cm_config::AgentConfig;
use crate::cm_internal::tools::{PreparedRunCommand, prepare_run_command_for_pty_spawn};
use crate::cm_sse_protocol::sse::SseEncoder;

const MAX_SESSIONS: usize = 8;
/// 连续无可读数据达到此时长后，认为本轮 PTY 输出暂告一段落。
const IDLE_DRAIN: Duration = Duration::from_secs(30);
const POLL_SLEEP: Duration = Duration::from_millis(25);
/// 单次 PTY 写入等待全部字节排空的最长时长（非阻塞 master 上遇 `EAGAIN` 时短睡重试）。
const WRITE_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

static NEXT_SESSION_N: AtomicU64 = AtomicU64::new(1);

static SESSIONS: LazyLock<Mutex<HashMap<String, PtySession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct PtySession {
    master: OwnedFd,
    child: Pid,
    cols: u16,
    rows: u16,
}

struct DrainIdleCfg {
    wall: Duration,
    max_capture: usize,
    child_pid: Option<Pid>,
}

#[derive(Debug, Deserialize)]
struct TerminalSessionArgs {
    action: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    signal: Option<i32>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

fn alloc_session_id() -> String {
    format!("pty{}", NEXT_SESSION_N.fetch_add(1, Ordering::Relaxed))
}

fn normalize_action(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn session_id_trimmed(a: &TerminalSessionArgs) -> Option<&str> {
    a.session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 会话表中子进程已退出或内核已无该子进程：移除条目（`waitpid` 非阻塞收尸或判死）。
fn remove_session_if_child_exited(sid: &str) -> Result<bool, String> {
    let mut guard = SESSIONS.lock().map_err(|_| "会话表锁中毒".to_string())?;
    let Some(sess) = guard.get(sid) else {
        return Ok(false);
    };
    let pid = sess.child;
    match waitpid(Some(pid), Some(WaitPidFlag::WNOHANG)) {
        Ok(WaitStatus::StillAlive) => Ok(false),
        Ok(_) => {
            guard.remove(sid);
            Ok(true)
        }
        Err(Errno::ECHILD) => {
            guard.remove(sid);
            Ok(true)
        }
        Err(e) => Err(format!("waitpid: {e}")),
    }
}

fn prune_all_defunct_sessions() -> usize {
    let Ok(mut guard) = SESSIONS.lock() else {
        return 0;
    };
    let keys: Vec<String> = guard.keys().cloned().collect();
    let mut removed = 0usize;
    for k in keys {
        let Some(sess) = guard.get(&k) else {
            continue;
        };
        let pid = sess.child;
        match waitpid(Some(pid), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {}
            Ok(_) | Err(Errno::ECHILD) => {
                guard.remove(&k);
                removed = removed.saturating_add(1);
            }
            Err(_) => {}
        }
    }
    removed
}

#[derive(Debug)]
enum MasterWriteOutcome {
    Ok,
    BrokenPipe,
    Err(String),
}

fn write_master_for_sid(sid: &str, to_write: &[u8]) -> MasterWriteOutcome {
    // 锁内仅 dup master，写出在锁外进行：非阻塞 master 上遇 `EAGAIN` 需短睡重试，不应持锁。
    let master = {
        let guard = match SESSIONS.lock() {
            Ok(g) => g,
            Err(_) => return MasterWriteOutcome::Err("会话表锁中毒".to_string()),
        };
        let Some(sess) = guard.get(sid) else {
            return MasterWriteOutcome::Err("会话已丢失".to_string());
        };
        match dup(sess.master.as_fd()) {
            Ok(d) => d,
            Err(e) => return MasterWriteOutcome::Err(format!("dup PTY 失败: {e}")),
        }
    };
    let deadline = Instant::now() + WRITE_DRAIN_TIMEOUT;
    let mut written = 0usize;
    while written < to_write.len() {
        match write(master.as_fd(), &to_write[written..]) {
            // 0 字节：对端已关闭。
            Ok(0) => return MasterWriteOutcome::BrokenPipe,
            Ok(n) => written = written.saturating_add(n),
            Err(Errno::EPIPE) => return MasterWriteOutcome::BrokenPipe,
            Err(Errno::EINTR) => {
                // 被信号打断：重试但受 deadline 约束，避免信号风暴下空转。
                if Instant::now() >= deadline {
                    return MasterWriteOutcome::Err(format!(
                        "PTY 写入被信号中断（已写 {written}/{} 字节）",
                        to_write.len()
                    ));
                }
            }
            Err(Errno::EAGAIN) => {
                // 非阻塞 master 的写缓冲已满：短睡后重试，直至超时。
                if Instant::now() >= deadline {
                    return MasterWriteOutcome::Err(format!(
                        "PTY 写入未排空（已写 {written}/{} 字节）",
                        to_write.len()
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return MasterWriteOutcome::Err(format!("{e}")),
        }
    }
    MasterWriteOutcome::Ok
}

fn cancel_is_set(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

/// 每轮读取开始前的停止判定：取消优先于墙上时钟（取消须走会话回收路径）。
fn round_stop_reason(cancel: Option<&Arc<AtomicBool>>, deadline: Instant) -> Option<DrainStop> {
    if cancel_is_set(cancel) {
        return Some(DrainStop::Cancelled);
    }
    if Instant::now() >= deadline {
        return Some(DrainStop::Wall);
    }
    None
}

/// `drain_until_idle` 一轮结束的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainStop {
    /// 子进程退出 / master 半关闭（`EIO` 等）——会话已终止。
    Eof,
    /// 连续静默达 `IDLE_DRAIN`——会话仍打开。
    Idle,
    /// 墙上时钟到期——会话仍打开。
    Wall,
    /// 上层取消——调用方应关闭会话。
    Cancelled,
}

#[allow(clippy::too_many_arguments)]
async fn drain_until_idle(
    dup_master: OwnedFd,
    cfg: DrainIdleCfg,
    seq: &mut u64,
    tool_call_id: &str,
    out: Option<&Sender<String>>,
    mirror: Option<&crate::cm_sse_protocol::sse::SseControlMirror>,
    encoder: &dyn SseEncoder,
    cancel: Option<&Arc<AtomicBool>>,
) -> (String, DrainStop) {
    let DrainIdleCfg {
        wall,
        max_capture,
        child_pid,
    } = cfg;
    let dup_arc = Arc::new(dup_master);
    let deadline = Instant::now() + wall;
    let mut acc = String::new();
    let mut empty_streak = Duration::ZERO;

    loop {
        if let Some(stop) = round_stop_reason(cancel, deadline) {
            return (acc, stop);
        }
        let mut read_any = false;
        loop {
            if cancel_is_set(cancel) {
                return (acc, DrainStop::Cancelled);
            }
            let d = Arc::clone(&dup_arc);
            let r = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 8192];
                read(d.as_ref(), &mut buf).map(|n| (buf, n))
            })
            .await;
            match r {
                Ok(Ok((buf, n))) => {
                    if n == 0 {
                        return (acc, DrainStop::Eof);
                    }
                    read_any = true;
                    empty_streak = Duration::ZERO;
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    emit_tool_chunk(seq, tool_call_id, chunk.as_ref(), out, mirror, encoder).await;
                    push_truncated(&mut acc, chunk.as_ref(), max_capture);
                }
                Ok(Err(e)) => {
                    if e == Errno::EAGAIN {
                        break;
                    }
                    // Master 半关闭、slave 挂断等：Linux 上常见 EIO；其余错误亦结束本轮以免悬挂会话。
                    return (acc, DrainStop::Eof);
                }
                Err(_) => break,
            }
        }
        if child_pid.is_some_and(child_gone_after_poll) {
            return (acc, DrainStop::Eof);
        }
        if !read_any {
            empty_streak += POLL_SLEEP;
            if empty_streak >= IDLE_DRAIN {
                return (acc, DrainStop::Idle);
            }
        }
        tokio::time::sleep(POLL_SLEEP).await;
    }
}

async fn kill_session_and_wait(child: Pid) {
    let _ = kill(child, Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(90)).await;
    let _ = kill(child, Signal::SIGKILL);
    reap_child_background(child).await;
}

/// 同步兜底关闭会话：摘除条目、`SIGKILL` 子进程并后台收尸（供 `Drop` 使用，不做异步等待）。
fn close_session_sync(sid: &str) {
    if let Some(pid) = remove_session_pid_skip_on_poison(sid) {
        let _ = kill(pid, Signal::SIGKILL);
        std::thread::spawn(move || reap_child_blocking(pid));
    }
}

/// 异步关闭会话：摘除条目后走 `SIGTERM`→`SIGKILL` 等待收尸。
async fn close_session_by_id(sid: &str) -> Result<(), String> {
    if let Some(pid) = remove_session_pid_trusting_lock(sid)? {
        kill_session_and_wait(pid).await;
    }
    Ok(())
}

/// 上层取消后收束正文：关闭会话并附注提示。
async fn close_session_after_cancel(sid: &str, captured: String) -> String {
    let mut body = captured;
    match close_session_by_id(sid).await {
        Ok(()) => body.push_str(&format!("\n\n已按取消请求关闭会话 `{sid}`（PTY 已清理）。")),
        Err(e) => body.push_str(&format!("\n\n取消后关闭会话 `{sid}` 失败：{e}")),
    }
    body
}

/// 会话清理守卫：exec 路径在会话存在后 `armed`，正常返回前 `disarm`。
///
/// 若外层墙钟（`parallel_tool_wall_timeout_secs`）或取消导致本工具 future 被 drop，
/// 尚未 `disarm` 的守卫在 `Drop` 中同步关闭会话，避免 PTY 长期占满 8 路上限。
struct ExecSessionGuard {
    sid: Option<String>,
}

impl ExecSessionGuard {
    fn armed(sid: &str) -> Self {
        Self {
            sid: Some(sid.to_string()),
        }
    }

    fn disarm(&mut self) {
        self.sid = None;
    }
}

impl Drop for ExecSessionGuard {
    fn drop(&mut self) {
        if let Some(sid) = self.sid.take() {
            close_session_sync(&sid);
        }
    }
}

fn run_command_json_from_exec_fields(command: &str, args: &[String]) -> String {
    serde_json::json!({
        "command": command,
        "args": args,
    })
    .to_string()
}

fn sessions_lock() -> Result<std::sync::MutexGuard<'static, HashMap<String, PtySession>>, String> {
    SESSIONS
        .lock()
        .map_err(|_| "错误：会话表锁中毒。".to_string())
}

fn remove_session_pid_skip_on_poison(sid: &str) -> Option<Pid> {
    let mut guard = SESSIONS.lock().ok()?;
    guard.remove(sid).map(|s| s.child)
}

fn remove_session_pid_trusting_lock(sid: &str) -> Result<Option<Pid>, String> {
    let mut guard = sessions_lock()?;
    Ok(guard.remove(sid).map(|s| s.child))
}

fn terminal_action_list() -> String {
    let pruned = prune_all_defunct_sessions();
    let guard = match sessions_lock() {
        Ok(g) => g,
        Err(e) => return e,
    };
    if guard.is_empty() {
        return if pruned > 0 {
            format!("当前无活动会话。（已清理 {pruned} 个已退出会话条目）")
        } else {
            "当前无活动的交互式终端会话。".to_string()
        };
    }
    let mut rows: Vec<String> = Vec::new();
    for (id, s) in guard.iter() {
        rows.push(format!("{} pid={} 终端 {}×{}", id, s.child, s.cols, s.rows));
    }
    rows.sort();
    let mut body = format!("活动会话 {} 个：\n{}", guard.len(), rows.join("\n"));
    if pruned > 0 {
        body.push_str(&format!("\n（列出前已清理 {pruned} 个已退出会话条目）"));
    }
    body
}

async fn terminal_action_close(a: &TerminalSessionArgs) -> String {
    let sid = match session_id_trimmed(a) {
        Some(s) => s.to_string(),
        None => return "错误：close 须提供 session_id。".to_string(),
    };
    let child = {
        let mut guard = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = guard.remove(&sid) else {
            return format!("错误：未知 session_id \"{sid}\"。");
        };
        sess.child
    };
    kill_session_and_wait(child).await;
    format!("会话 \"{sid}\" 已关闭。")
}

fn terminal_action_resize(a: &TerminalSessionArgs) -> String {
    let sid = match session_id_trimmed(a) {
        Some(s) => s.to_string(),
        None => return "错误：resize 须提供 session_id。".to_string(),
    };
    let cols = a.cols.unwrap_or(80);
    let rows = a.rows.unwrap_or(24);
    if cols == 0 || rows == 0 {
        return "错误：cols/rows 须为正整数。".to_string();
    }
    let mut guard = match sessions_lock() {
        Ok(g) => g,
        Err(e) => return e,
    };
    let Some(sess) = guard.get_mut(&sid) else {
        return format!("错误：未知 session_id \"{sid}\"。");
    };
    if let Err(e) = resize_session_master(&sess.master, cols, rows) {
        return e;
    }
    sess.cols = cols;
    sess.rows = rows;
    format!("会话 \"{sid}\" 已调整为 {}×{}。", cols, rows)
}

fn terminal_action_send_signal(a: &TerminalSessionArgs) -> String {
    let sid = match session_id_trimmed(a) {
        Some(s) => s.to_string(),
        None => return "错误：send_signal 须提供 session_id。".to_string(),
    };
    let sig_n = match a.signal {
        Some(s) => s,
        None => return "错误：send_signal 须提供 signal（整数）。".to_string(),
    };
    let sig = match Signal::try_from(sig_n) {
        Ok(s) => s,
        Err(_) => return format!("错误：无效 signal 编号 {sig_n}。"),
    };
    let (child, fg_pgid) = {
        let guard = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = guard.get(&sid) else {
            return format!("错误：未知 session_id \"{sid}\"。");
        };
        // 终端前台进程组：与 Ctrl-C 等信号语义一致的目标；取不到时回落到会话首进程。
        let fg = tcgetpgrp(sess.master.as_fd()).ok().map(Pid::as_raw);
        (sess.child, fg)
    };
    let (target, scope) = match fg_pgid {
        Some(pgid) if pgid > 0 => (Pid::from_raw(-pgid), format!("前台进程组 {pgid}")),
        _ => (child, format!("会话首进程 {child}")),
    };
    if let Err(e) = kill(target, sig) {
        return format!("错误：发送信号失败: {e}");
    }
    format!("已向会话 \"{sid}\" 的{scope}发送信号 {sig_n}。")
}

/// 通过向 PTY master 写入 `ETX`（`Ctrl-C`）中断会话前台进程组，语义等同终端按键。
async fn terminal_action_interrupt(a: &TerminalSessionArgs) -> String {
    let sid = match session_id_trimmed(a) {
        Some(s) => s.to_string(),
        None => return "错误：interrupt 须提供 session_id。".to_string(),
    };
    let sid_owned = sid.clone();
    let wres = tokio::task::spawn_blocking(move || write_master_for_sid(&sid_owned, &[0x03])).await;
    match wres {
        Ok(MasterWriteOutcome::Ok) => format!("已向会话 \"{sid}\" 发送 Ctrl-C（ETX）。"),
        Ok(MasterWriteOutcome::BrokenPipe) => {
            format!("错误：会话 \"{sid}\" 的 PTY 已断开，无法发送 Ctrl-C。")
        }
        Ok(MasterWriteOutcome::Err(msg)) => {
            format!("错误：向会话 \"{sid}\" 发送 Ctrl-C 失败：{msg}")
        }
        Err(_) => "错误：向会话发送 Ctrl-C 失败（任务异常）。".to_string(),
    }
}

/// exec 分支：`drain_until_idle` / SSE 共用字段。
struct TerminalStreamCtx<'a> {
    wall: Duration,
    max_capture: usize,
    seq: &'a mut u64,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    encoder: &'a dyn SseEncoder,
    cancel: Option<&'a Arc<AtomicBool>>,
}

async fn reap_removed_session_child_or_return_captured(sid: &str, captured: String) -> String {
    if let Some(pid) = remove_session_pid_skip_on_poison(sid) {
        reap_child_background(pid).await;
    }
    captured
}

async fn spawn_initial_write_failed_cleanup(sid: &str) -> Result<(), ()> {
    let pid_opt = {
        let mut guard = SESSIONS.lock().map_err(|_| ())?;
        guard.remove(sid).map(|s| s.child)
    };
    if let Some(pid) = pid_opt {
        reap_child_background(pid).await;
    }
    Ok(())
}

fn terminal_spawn_fork_session(
    _workspace: &Path,
    prepared: &PreparedRunCommand,
    cols: u16,
    rows: u16,
) -> Result<(Pid, String), String> {
    let mut guard = sessions_lock()?;
    if guard.len() >= MAX_SESSIONS {
        return Err(format!(
            "错误：交互式会话已达上限（{MAX_SESSIONS}），请先 close。"
        ));
    }
    let (child, master) = fork_pty_session(prepared, cols, rows)?;
    let sid = alloc_session_id();
    guard.insert(
        sid.clone(),
        PtySession {
            master,
            child,
            cols,
            rows,
        },
    );
    Ok((child, sid))
}

async fn terminal_spawn_stdin_write_user_err(sid: &str, msg: String) -> String {
    if spawn_initial_write_failed_cleanup(sid).await.is_err() {
        "错误：初始写入 PTY 失败且会话表锁中毒。".to_string()
    } else {
        msg
    }
}

async fn terminal_spawn_write_stdin_if_nonempty(sid: &str, input: Vec<u8>) -> Result<(), String> {
    if input.is_empty() {
        return Ok(());
    }
    let sid_owned = sid.to_string();
    let wres = tokio::task::spawn_blocking(move || write_master_for_sid(&sid_owned, &input)).await;
    match wres {
        Ok(MasterWriteOutcome::Ok) => Ok(()),
        Ok(MasterWriteOutcome::BrokenPipe) => Err(terminal_spawn_stdin_write_user_err(
            sid,
            "错误：初始写入失败（PTY 已断开），会话已清理。".to_string(),
        )
        .await),
        Ok(MasterWriteOutcome::Err(msg)) => Err(terminal_spawn_stdin_write_user_err(
            sid,
            format!("错误：初始写入 PTY 失败：{msg}"),
        )
        .await),
        Err(_) => Err(terminal_spawn_stdin_write_user_err(
            sid,
            "错误：初始写入 PTY 失败（任务异常）。".to_string(),
        )
        .await),
    }
}

async fn terminal_exec_resume_existing(
    sid: &str,
    a: &TerminalSessionArgs,
    ctx: &mut TerminalStreamCtx<'_>,
) -> String {
    match remove_session_if_child_exited(sid) {
        Ok(true) => return "错误：会话子进程已退出，条目已移除。".to_string(),
        Ok(false) => {}
        Err(e) => return e,
    }
    let (dup_fd, child_pid) = {
        let guard = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = guard.get(sid) else {
            return format!("错误：未知 session_id \"{sid}\"。");
        };
        let child_pid = sess.child;
        match dup(sess.master.as_fd()) {
            Ok(d) => (d, child_pid),
            Err(e) => return format!("错误：dup PTY 失败: {e}"),
        }
    };
    let input = a.input.clone().unwrap_or_default();
    if !input.is_empty() {
        let sid_owned = sid.to_string();
        let to_write = input.into_bytes();
        let wres =
            tokio::task::spawn_blocking(move || write_master_for_sid(&sid_owned, &to_write)).await;
        match wres {
            Ok(MasterWriteOutcome::Ok) => {}
            Ok(MasterWriteOutcome::BrokenPipe) => {
                match remove_session_pid_trusting_lock(sid) {
                    Ok(Some(pid)) => reap_child_background(pid).await,
                    Ok(None) => {}
                    Err(e) => return e,
                }
                return "错误：PTY 已断开（SIGPIPE/EPIPE，子进程可能已退出），会话已清理。"
                    .to_string();
            }
            Ok(MasterWriteOutcome::Err(msg)) => {
                return format!("错误：向 PTY 写入失败：{msg}");
            }
            Err(_) => return "错误：向 PTY 写入失败（任务异常）。".to_string(),
        }
    }
    let mut guard = ExecSessionGuard::armed(sid);
    let (captured, stop) = drain_until_idle(
        dup_fd,
        DrainIdleCfg {
            wall: ctx.wall,
            max_capture: ctx.max_capture,
            child_pid: Some(child_pid),
        },
        ctx.seq,
        ctx.tool_call_id,
        ctx.sse_out_tx,
        ctx.sse_control_mirror,
        ctx.encoder,
        ctx.cancel,
    )
    .await;
    guard.disarm();
    let captured = match stop {
        DrainStop::Eof => reap_removed_session_child_or_return_captured(sid, captured).await,
        DrainStop::Cancelled => close_session_after_cancel(sid, captured).await,
        DrainStop::Idle | DrainStop::Wall => captured,
    };
    let capped = captured.len() >= ctx.max_capture;
    let mut body = captured;
    if capped {
        body.push_str("\n…（正文已按 command_max_output_len 截断）");
    }
    body
}

async fn terminal_exec_spawn_new(
    workspace: &Path,
    a: &TerminalSessionArgs,
    cols: u16,
    rows: u16,
    allowed_commands: &[String],
    skip_arg_safety: bool,
    ctx: &mut TerminalStreamCtx<'_>,
) -> String {
    let cmd = match a
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(c) => c.to_string(),
        None => return "错误：新建 exec 会话须提供 command。".to_string(),
    };
    let args_vec = a.args.clone().unwrap_or_default();
    let rc_json = run_command_json_from_exec_fields(&cmd, &args_vec);
    let prepared = match prepare_run_command_for_pty_spawn(
        &rc_json,
        workspace,
        allowed_commands,
        skip_arg_safety,
    ) {
        Ok(p) => p,
        Err(e) => return e.extended_user_message(),
    };

    let (child, sid) = match terminal_spawn_fork_session(workspace, &prepared, cols, rows) {
        Ok(v) => v,
        Err(e) => return e,
    };

    // 从会话创建起 armed：若本 future 在初始写入 / drain 期间被 drop（外层超时/取消），Drop 兜底清理。
    let mut guard = ExecSessionGuard::armed(&sid);
    let input = a.input.clone().unwrap_or_default().into_bytes();
    if let Err(msg) = terminal_spawn_write_stdin_if_nonempty(&sid, input).await {
        return msg;
    }

    let (dup_fd, child_pid) = {
        let sessions = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = sessions.get(&sid) else {
            return "错误：会话尚未就绪。".to_string();
        };
        match dup(sess.master.as_fd()) {
            Ok(d) => (d, sess.child),
            Err(e) => return format!("错误：dup PTY 失败: {e}"),
        }
    };

    let (captured, stop) = drain_until_idle(
        dup_fd,
        DrainIdleCfg {
            wall: ctx.wall,
            max_capture: ctx.max_capture,
            child_pid: Some(child_pid),
        },
        ctx.seq,
        ctx.tool_call_id,
        ctx.sse_out_tx,
        ctx.sse_control_mirror,
        ctx.encoder,
        ctx.cancel,
    )
    .await;
    guard.disarm();
    let captured = match stop {
        DrainStop::Eof => reap_removed_session_child_or_return_captured(&sid, captured).await,
        DrainStop::Cancelled => close_session_after_cancel(&sid, captured).await,
        DrainStop::Idle | DrainStop::Wall => captured,
    };

    let capped = captured.len() >= ctx.max_capture;
    let mut body = captured;
    if matches!(stop, DrainStop::Idle | DrainStop::Wall) {
        body.push_str(&format!(
            "\n\n会话 `{sid}` 仍打开（子 PID {child}）；后续可用 {{ \"action\": \"exec\", \"session_id\": \"{sid}\", \"input\": \"…\" }} 继续交互。"
        ));
    }
    if capped {
        body.push_str("\n…（正文已按 command_max_output_len 截断）");
    }
    body
}

struct TerminalActionExecArgs<'a> {
    workspace: &'a Path,
    a: &'a TerminalSessionArgs,
    wall: Duration,
    max_cap: usize,
    seq: &'a mut u64,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    allowed_commands: &'a [String],
    skip_arg_safety: bool,
    encoder: Option<&'a dyn SseEncoder>,
    cancel: Option<Arc<AtomicBool>>,
}

async fn terminal_action_exec(args: TerminalActionExecArgs<'_>) -> String {
    let TerminalActionExecArgs {
        workspace,
        a,
        wall,
        max_cap,
        seq,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        allowed_commands,
        skip_arg_safety,
        encoder,
        cancel,
    } = args;
    let cols = a.cols.unwrap_or(80);
    let rows = a.rows.unwrap_or(24);
    if cols == 0 || rows == 0 {
        return "错误：cols/rows 须为正整数。".to_string();
    }
    let encoder_ref: &dyn SseEncoder = encoder.unwrap_or(&crate::cm_sse_protocol::sse::V2Encoder);
    let mut ctx = TerminalStreamCtx {
        wall,
        max_capture: max_cap,
        seq,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
        encoder: encoder_ref,
        cancel: cancel.as_ref(),
    };
    if let Some(sid) = session_id_trimmed(a) {
        terminal_exec_resume_existing(sid, a, &mut ctx).await
    } else {
        terminal_exec_spawn_new(
            workspace,
            a,
            cols,
            rows,
            allowed_commands,
            skip_arg_safety,
            &mut ctx,
        )
        .await
    }
}

/// Linux：解析 `terminal_session` JSON，维护 PTY 会话表并发 SSE chunk。
#[allow(clippy::too_many_arguments)]
pub async fn execute_terminal_session(
    cfg: &Arc<AgentConfig>,
    workspace: &Path,
    args_json: &str,
    tool_call_id: &str,
    sse: TerminalSseSink<'_>,
    allowed_commands: &[String],
    skip_arg_safety: bool,
    cancel: Option<Arc<AtomicBool>>,
) -> String {
    let TerminalSseSink {
        out_tx: sse_out_tx,
        control_mirror: sse_control_mirror,
        encoder,
    } = sse;
    let a: TerminalSessionArgs = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("错误：参数 JSON 无效: {e}"),
    };
    let action = normalize_action(&a.action);
    let wall = Duration::from_secs(cfg.command_exec.command_timeout_secs.max(1));
    let max_cap = cfg.command_exec.command_max_output_len.max(1024);
    let mut seq: u64 = 0;

    match action.as_str() {
        "list" => terminal_action_list(),
        "close" => terminal_action_close(&a).await,
        "resize" => terminal_action_resize(&a),
        "send_signal" => terminal_action_send_signal(&a),
        "interrupt" => terminal_action_interrupt(&a).await,
        "exec" => {
            terminal_action_exec(TerminalActionExecArgs {
                workspace,
                a: &a,
                wall,
                max_cap,
                seq: &mut seq,
                tool_call_id,
                sse_out_tx,
                sse_control_mirror,
                allowed_commands,
                skip_arg_safety,
                encoder,
                cancel,
            })
            .await
        }
        _ => format!(
            "错误：未知 action \"{}\"；应为 exec / send_signal / interrupt / resize / list / close。",
            a.action
        ),
    }
}
