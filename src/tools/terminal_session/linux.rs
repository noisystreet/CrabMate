//! Linux PTY **`terminal_session`**：`forkpty` + 会话表；输出经 SSE **`tool_output_chunk`** 增量下发。

use std::collections::HashMap;
use std::ffi::CString;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use libc::ioctl;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, chdir, dup, execvp, read, write};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::sse::{SsePayload, ToolOutputChunkBody, send_sse_control_payload_optional};
use crate::tools::command::{self, PreparedRunCommand};

const MAX_SESSIONS: usize = 8;
/// 连续无可读数据达到此时长后，认为本轮 PTY 输出暂告一段落。
const IDLE_DRAIN: Duration = Duration::from_secs(30);
const POLL_SLEEP: Duration = Duration::from_millis(25);

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

fn exec_strings_for_prepared(p: &PreparedRunCommand) -> Result<(CString, Vec<CString>), String> {
    let prog = if let Some(ep) = &p.exec_path {
        CString::new(ep.as_os_str().as_bytes()).map_err(|e| e.to_string())?
    } else {
        CString::new(p.cmd_name.as_bytes()).map_err(|e| e.to_string())?
    };
    let mut argv = Vec::with_capacity(1 + p.cmd_args.len());
    argv.push(CString::new(p.cmd_raw.as_str()).map_err(|e| e.to_string())?);
    for x in &p.cmd_args {
        argv.push(CString::new(x.as_str()).map_err(|e| e.to_string())?);
    }
    Ok((prog, argv))
}

fn set_nonblocking(master: &OwnedFd) -> Result<(), String> {
    let bits = fcntl(master.as_fd(), FcntlArg::F_GETFL).map_err(|e| format!("fcntl GETFL: {e}"))?;
    let flags = OFlag::from_bits_truncate(bits);
    fcntl(master.as_fd(), FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|e| format!("fcntl SETFL: {e}"))?;
    Ok(())
}

fn push_truncated(acc: &mut String, chunk: &str, max_len: usize) {
    let remain = max_len.saturating_sub(acc.len());
    if remain == 0 {
        return;
    }
    if chunk.len() <= remain {
        acc.push_str(chunk);
    } else {
        let mut end = remain;
        while end > 0 && !chunk.is_char_boundary(end) {
            end -= 1;
        }
        acc.push_str(&chunk[..end]);
    }
}

async fn emit_tool_chunk(
    seq: &mut u64,
    tool_call_id: &str,
    text: &str,
    out: Option<&Sender<String>>,
    mirror: Option<&crate::sse::SseControlMirror>,
) {
    if text.is_empty() {
        return;
    }
    *seq = seq.saturating_add(1);
    let body = ToolOutputChunkBody {
        tool_call_id: tool_call_id.to_string(),
        name: Some("terminal_session".to_string()),
        seq: *seq,
        chunk: text.to_string(),
        stream: Some("combined".to_string()),
    };
    let _ = send_sse_control_payload_optional(
        out,
        mirror,
        SsePayload::ToolOutputChunk {
            tool_output_chunk: body,
        },
        "terminal_session::pty_chunk",
    )
    .await;
}

/// 从 dup 的 master 端读 PTY，直到空闲或墙上时钟；返回本轮捕获文本（用于 tool_result 正文）。
/// 子进程已退出或已不可 `wait`（`ECHILD`）：可能已由本次 `WNOHANG` 收尸。
fn child_gone_after_poll(pid: Pid) -> bool {
    match waitpid(Some(pid), Some(WaitPidFlag::WNOHANG)) {
        Ok(WaitStatus::StillAlive) => false,
        Ok(_) => true,
        Err(Errno::ECHILD) => true,
        Err(_) => false,
    }
}

fn reap_child_blocking(pid: Pid) {
    match waitpid(Some(pid), None) {
        Ok(_) | Err(Errno::ECHILD) => {}
        Err(_) => {}
    }
}

async fn reap_child_background(pid: Pid) {
    let _ = tokio::task::spawn_blocking(move || reap_child_blocking(pid)).await;
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
    let guard = match SESSIONS.lock() {
        Ok(g) => g,
        Err(_) => return MasterWriteOutcome::Err("会话表锁中毒".to_string()),
    };
    let Some(sess) = guard.get(sid) else {
        return MasterWriteOutcome::Err("会话已丢失".to_string());
    };
    match write(sess.master.as_fd(), to_write) {
        Ok(_) => MasterWriteOutcome::Ok,
        Err(Errno::EPIPE) => MasterWriteOutcome::BrokenPipe,
        Err(e) => MasterWriteOutcome::Err(format!("{e}")),
    }
}

async fn drain_until_idle(
    dup_master: OwnedFd,
    cfg: DrainIdleCfg,
    seq: &mut u64,
    tool_call_id: &str,
    out: Option<&Sender<String>>,
    mirror: Option<&crate::sse::SseControlMirror>,
) -> (String, bool) {
    let DrainIdleCfg {
        wall,
        max_capture,
        child_pid,
    } = cfg;
    let dup_arc = Arc::new(dup_master);
    let deadline = Instant::now() + wall;
    let mut acc = String::new();
    let mut empty_streak = Duration::ZERO;
    let mut saw_eof = false;

    while Instant::now() < deadline {
        let mut read_any = false;
        loop {
            let d = Arc::clone(&dup_arc);
            let r = tokio::task::spawn_blocking(move || {
                let mut buf = [0u8; 8192];
                read(d.as_ref(), &mut buf).map(|n| (buf, n))
            })
            .await;
            match r {
                Ok(Ok((buf, n))) => {
                    if n == 0 {
                        saw_eof = true;
                        break;
                    }
                    read_any = true;
                    empty_streak = Duration::ZERO;
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    emit_tool_chunk(seq, tool_call_id, chunk.as_ref(), out, mirror).await;
                    push_truncated(&mut acc, chunk.as_ref(), max_capture);
                }
                Ok(Err(e)) => {
                    if e == Errno::EAGAIN {
                        break;
                    }
                    // Master 半关闭、slave 挂断等：Linux 上常见 EIO；其余错误亦结束本轮以免悬挂会话。
                    saw_eof = true;
                    break;
                }
                Err(_) => break,
            }
        }
        if saw_eof {
            break;
        }
        if child_pid.is_some_and(child_gone_after_poll) {
            saw_eof = true;
            break;
        }
        if read_any {
            tokio::time::sleep(POLL_SLEEP).await;
            continue;
        }
        empty_streak += POLL_SLEEP;
        if empty_streak >= IDLE_DRAIN {
            break;
        }
        tokio::time::sleep(POLL_SLEEP).await;
    }
    (acc, saw_eof)
}

fn fork_pty_session(
    prepared: &PreparedRunCommand,
    cols: u16,
    rows: u16,
) -> Result<(Pid, OwnedFd), String> {
    let (prog, argv) = exec_strings_for_prepared(prepared)?;
    let ws = Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    // SAFETY: `forkpty` 仅在此处分叉；子进程尽快 `exec`/`_exit`，不做额外分配。
    let pair = unsafe { forkpty(Some(&ws), None).map_err(|e| format!("forkpty 失败: {e}"))? };

    match pair {
        ForkptyResult::Child => {
            let _ = chdir(prepared.effective_working_dir.as_path());
            let _ = execvp(&prog, &argv);
            unsafe { libc::_exit(127) };
        }
        ForkptyResult::Parent { child, master } => {
            set_nonblocking(&master)?;
            Ok((child, master))
        }
    }
}

fn resize_session_master(master: &OwnedFd, cols: u16, rows: u16) -> Result<(), String> {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `TIOCSWINSZ`  ioctl，第三个参数为 winsize 指针。
    let r = unsafe { ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
    if r != 0 {
        return Err(format!("ioctl TIOCSWINSZ 失败: {:?}", Errno::last()));
    }
    Ok(())
}

async fn kill_session_and_wait(child: Pid) {
    let _ = kill(child, Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(90)).await;
    let _ = kill(child, Signal::SIGKILL);
    reap_child_background(child).await;
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
    let child = {
        let guard = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = guard.get(&sid) else {
            return format!("错误：未知 session_id \"{sid}\"。");
        };
        sess.child
    };
    if let Err(e) = kill(child, sig) {
        return format!("错误：发送信号失败: {e}");
    }
    format!("已向会话 \"{sid}\" 的进程发送信号 {sig_n}。")
}

/// exec 分支：`drain_until_idle` / SSE 共用字段。
struct TerminalStreamCtx<'a> {
    wall: Duration,
    max_capture: usize,
    seq: &'a mut u64,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a Sender<String>>,
    sse_control_mirror: Option<&'a crate::sse::SseControlMirror>,
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
    let (captured, eof) = drain_until_idle(
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
    )
    .await;
    let captured = if eof {
        reap_removed_session_child_or_return_captured(sid, captured).await
    } else {
        captured
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
    let prepared =
        match command::prepare_run_command_for_pty_spawn(&rc_json, workspace, allowed_commands) {
            Ok(p) => p,
            Err(e) => return e.extended_user_message(),
        };

    let (child, sid) = match terminal_spawn_fork_session(workspace, &prepared, cols, rows) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let input = a.input.clone().unwrap_or_default().into_bytes();
    if let Err(msg) = terminal_spawn_write_stdin_if_nonempty(&sid, input).await {
        return msg;
    }

    let (dup_fd, child_pid) = {
        let guard = match sessions_lock() {
            Ok(g) => g,
            Err(e) => return e,
        };
        let Some(sess) = guard.get(&sid) else {
            return "错误：会话尚未就绪。".to_string();
        };
        match dup(sess.master.as_fd()) {
            Ok(d) => (d, sess.child),
            Err(e) => return format!("错误：dup PTY 失败: {e}"),
        }
    };

    let (captured, eof_flag) = drain_until_idle(
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
    )
    .await;
    let captured = if eof_flag {
        reap_removed_session_child_or_return_captured(&sid, captured).await
    } else {
        captured
    };

    let capped = captured.len() >= ctx.max_capture;
    let mut body = captured;
    if !eof_flag {
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
    sse_control_mirror: Option<&'a crate::sse::SseControlMirror>,
    allowed_commands: &'a [String],
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
    } = args;
    let cols = a.cols.unwrap_or(80);
    let rows = a.rows.unwrap_or(24);
    if cols == 0 || rows == 0 {
        return "错误：cols/rows 须为正整数。".to_string();
    }
    let mut ctx = TerminalStreamCtx {
        wall,
        max_capture: max_cap,
        seq,
        tool_call_id,
        sse_out_tx,
        sse_control_mirror,
    };
    if let Some(sid) = session_id_trimmed(a) {
        terminal_exec_resume_existing(sid, a, &mut ctx).await
    } else {
        terminal_exec_spawn_new(workspace, a, cols, rows, allowed_commands, &mut ctx).await
    }
}

/// Linux：解析 `terminal_session` JSON，维护 PTY 会话表并发 SSE chunk。
pub(crate) async fn execute_terminal_session(
    cfg: &Arc<AgentConfig>,
    workspace: &Path,
    args_json: &str,
    tool_call_id: &str,
    sse_out_tx: Option<&Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    allowed_commands: &[String],
) -> String {
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
            })
            .await
        }
        _ => format!(
            "错误：未知 action \"{}\"；应为 exec / send_signal / resize / list / close。",
            a.action
        ),
    }
}
