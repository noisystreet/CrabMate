//! 整组终止：`SIGTERM` → 宽限 → `SIGKILL`，目标是**进程组**而非单个 pid。
//!
//! `serve` 会起工具子进程（`clang-format`、`cmake` 等），只杀 pid 会留下孤儿，
//! 因此子进程以 `process_group(0)` 自成一个组，`down` 对整组发信号。
//!
//! **防自杀**：若目标组恰好是 ctl 自己所在的组（脚本里同组启动等极端情形），
//! 降级为只对单 pid 发信号，否则会把自己和调用方脚本一起打死。

use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;

use super::{CtlError, identity};

/// 实际的信号投递范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminateMode {
    /// 对整个进程组（正常情形）。
    Group,
    /// 仅单 pid（目标组与 ctl 自身同组时的降级）。
    SinglePid,
}

impl TerminateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::SinglePid => "single_pid",
        }
    }
}

/// 终止结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminateSummary {
    pub mode: TerminateMode,
    /// 是否动用了 `SIGKILL`（即宽限期内没退出）。
    pub forced: bool,
}

/// 选择投递范围：目标组等于 ctl 自身组时降级为单 pid。
fn pick_mode(pid: u32, pgid: Option<i32>) -> TerminateMode {
    let target = pgid.unwrap_or(pid as i32);
    if nix::unistd::getpgrp() == Pid::from_raw(target) {
        TerminateMode::SinglePid
    } else {
        TerminateMode::Group
    }
}

fn send(
    mode: TerminateMode,
    pid: u32,
    pgid: Option<i32>,
    sig: Signal,
) -> Result<(), CtlError> {
    let res = match mode {
        TerminateMode::Group => {
            let group = Pid::from_raw(pgid.unwrap_or(pid as i32));
            killpg(group, sig)
        }
        TerminateMode::SinglePid => kill(Pid::from_raw(pid as i32), sig),
    };
    match res {
        // 进程/组已不存在：终止目标已达成
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(e) => Err(CtlError::general(
            "signal_failed",
            format!("向 {} 发 {sig} 失败：{e}", mode.as_str()),
        )),
    }
}

fn wait_gone(pid: u32, budget: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if !identity::pid_alive(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll.min(Duration::from_millis(100)));
    }
}

/// 终止实例进程；调用方必须先通过 [`identity::verify`] 确认身份。
///
/// 返回 `forced = true` 表示宽限期不够、动用了 `SIGKILL`（可能是 serve 阻塞在
/// 关闭流程上，值得在 `last_error` 里提示）。
pub fn terminate(
    pid: u32,
    pgid: Option<i32>,
    grace: Duration,
    poll: Duration,
) -> Result<TerminateSummary, CtlError> {
    let mode = pick_mode(pid, pgid);

    // pid 0 是非法的目标；超出 i32 范围的值转 `Pid` 会溢出成负数，内核会报
    // EINVAL 而不是 ESRCH —— 都不可能是真实的 serve，直接视为「已达成」。
    if pid == 0 || pid > i32::MAX as u32 {
        return Ok(TerminateSummary { mode, forced: false });
    }

    let mut forced = false;

    send(mode, pid, pgid, Signal::SIGTERM)?;
    if !wait_gone(pid, grace, poll) {
        send(mode, pid, pgid, Signal::SIGKILL)?;
        forced = true;
        // SIGKILL 后给内核一点时间回收，便于上层确认
        let _ = wait_gone(pid, Duration::from_millis(500), poll);
    }

    Ok(TerminateSummary { mode, forced })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_sleep_in_own_group() -> std::process::Child {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30");
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.spawn().expect("应能启动 sleep")
    }

    #[test]
    fn self_group_degrades_to_single_pid() {
        // 注意：`getpgrp()` 是**进程组 id**，不等于进程自身 pid。
        // 真正会自杀的情形是「记录的 pgid == ctl 自己的 pgid」。
        let me = std::process::id();
        let my_group = nix::unistd::getpgrp().as_raw();
        assert_eq!(
            pick_mode(me, Some(my_group)),
            TerminateMode::SinglePid,
            "目标组等于自身组时必须降级"
        );
    }

    #[test]
    fn foreign_group_targets_group() {
        // 目标组与自身不同 → 按组投递（真正会杀进程，故只测判定逻辑）
        let me = std::process::id();
        let other = nix::unistd::getpgrp().as_raw() + 1_000_000;
        assert_eq!(pick_mode(me, Some(other)), TerminateMode::Group);
    }

    #[test]
    fn terminates_child_process_group() {
        let child = spawn_sleep_in_own_group();
        let pid = child.id();
        // 等它真正起来
        std::thread::sleep(Duration::from_millis(150));
        assert!(identity::pid_alive(pid), "子进程应存活");

        let summary = terminate(
            pid,
            Some(pid as i32),
            Duration::from_secs(3),
            Duration::from_millis(20),
        )
        .expect("终止应成功");

        assert_eq!(summary.mode, TerminateMode::Group);
        assert!(!identity::pid_alive(pid), "终止后不应存活");
        // 让 std 回收僵尸
        let mut child = child;
        let _ = child.wait();
    }

    #[test]
    fn terminate_is_idempotent_for_reaped_pid() {
        // 真实语义：对「已退出且已回收」的实例再 down，必须成功而不是报错
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("应能启动 true");
        let pid = child.id();
        let _ = child.wait(); // 回收，pid 不再存在
        assert!(!identity::pid_alive(pid), "回收后应判不活");

        let summary = terminate(
            pid,
            Some(pid as i32),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .expect("对已消失的 pid 应幂等成功");
        assert!(!summary.forced);
    }

    #[test]
    fn terminate_ignores_out_of_range_pid() {
        // 超出 i32 的 pid 不可能是真实进程；不能因此报 signal_failed
        let summary = terminate(
            u32::MAX,
            Some(1),
            Duration::from_millis(10),
            Duration::from_millis(5),
        )
        .expect("越界 pid 应视为已达成");
        assert!(!summary.forced);
    }
}
