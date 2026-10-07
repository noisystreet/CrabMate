//! PTY 底层辅助：`forkpty`、`TIOCSWINSZ`、非阻塞设置与子进程收尸。

use std::ffi::CString;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;

use libc::ioctl;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, chdir, execvp};

use crate::cm_internal::tools::PreparedRunCommand;

pub(super) fn exec_strings_for_prepared(
    p: &PreparedRunCommand,
) -> Result<(CString, Vec<CString>), String> {
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

pub(super) fn set_nonblocking(master: &OwnedFd) -> Result<(), String> {
    let bits = fcntl(master.as_fd(), FcntlArg::F_GETFL).map_err(|e| format!("fcntl GETFL: {e}"))?;
    let flags = OFlag::from_bits_truncate(bits);
    fcntl(master.as_fd(), FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|e| format!("fcntl SETFL: {e}"))?;
    Ok(())
}

pub(super) fn fork_pty_session(
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

pub(super) fn resize_session_master(master: &OwnedFd, cols: u16, rows: u16) -> Result<(), String> {
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

/// 子进程已退出或已不可 `wait`（`ECHILD`）：可能已由本次 `WNOHANG` 收尸。
pub(super) fn child_gone_after_poll(pid: Pid) -> bool {
    match waitpid(Some(pid), Some(WaitPidFlag::WNOHANG)) {
        Ok(WaitStatus::StillAlive) => false,
        Ok(_) => true,
        Err(Errno::ECHILD) => true,
        Err(_) => false,
    }
}

pub(super) fn reap_child_blocking(pid: Pid) {
    match waitpid(Some(pid), None) {
        Ok(_) | Err(Errno::ECHILD) => {}
        Err(_) => {}
    }
}

pub(super) async fn reap_child_background(pid: Pid) {
    let _ = tokio::task::spawn_blocking(move || reap_child_blocking(pid)).await;
}
