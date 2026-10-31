//! 观测：把「记录里的 state」与「真实进程事实」合成为对外展示的状态。
//!
//! `stale` **不落盘**：它是读取时派生的事实（pid 在但连不上 / 身份不符），
//! 若写进文件会与观察者竞争出两个「真相」。落盘的只有
//! `starting` / `ready` / `failed` / `stopped` 四态。
//!
//! 映射矩阵（与 `docs/命令行与路由.md` 一致）：
//!
//! | 记录 state | pid 存活 | 身份 | TCP | Observed |
//! | --- | --- | --- | --- | --- |
//! | starting | 否 | — | — | failed |
//! | starting | 是 | ok | 否 | starting |
//! | starting | 是 | ok | 是 | ready |
//! | ready | 否 | — | — | stopped |
//! | ready | 是 | ok | 是 | ready（desired=stopped 时 held） |
//! | ready | 是 | ok | 否 | **stale** |
//! | ready | 是 | mismatch | — | **stale** |
//! | failed / stopped | — | — | — | 原值 |

use std::net::TcpStream;
use std::time::Duration;

use super::identity::{self, Identity};
use super::record::{DesiredState, InstanceRecord, InstanceState};
use super::readiness;

/// 对外观测态（比落盘四态多 `stale` / `held`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    Ready,
    Starting,
    /// 记录说在跑，但连不上或进程身份不符。
    Stale,
    Stopped,
    Failed,
    /// 进程仍在跑，但 `desired_state = stopped`（被 `down` 标记过）。
    Held,
}

impl Observed {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Starting => "starting",
            Self::Stale => "stale",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Held => "held",
        }
    }

    /// `prune` 是否可以清掉该类记录。
    pub fn is_prunable(self) -> bool {
        matches!(self, Self::Stale | Self::Stopped | Self::Failed)
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

/// 一次观测的完整结果（`ls --json` 与 `status` 都用它）。
#[derive(Debug, Clone)]
pub struct Probe {
    pub observed: Observed,
    pub identity: Identity,
    pub pid_alive: bool,
    pub tcp_ok: bool,
}

impl Probe {
    /// 记录指向的进程**确实还在跑，且身份校验通过**。
    ///
    /// 两者同时为真时，状态文件里的 `pid` 是那个进程在 ctl 视野中的唯一地址：删掉
    /// 记录或把它覆盖成别的 pid，进程就成孤儿（`down` 报实例不存在、`ls` 看不见、
    /// `supervise` 不回收）。所以 `prune` / `up` 用本谓词判「能不能动记录」，而不是
    /// 用 `Observed`：`Observed::Stale` 把「PID 被复用（该清）」与「进程活着但端口
    /// 不通（不能清）」混成了一类，见 `super::guard`。
    pub fn process_is_live_and_ours(&self) -> bool {
        self.pid_alive && self.identity.is_match()
    }
}

fn probe_tcp(rec: &InstanceRecord) -> bool {
    let Some(port) = rec.port else {
        return false;
    };
    let Ok(addr) = readiness::parse_addr("127.0.0.1", port) else {
        return false;
    };
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
}

/// 观测一个实例（只读，不发任何信号）。
pub fn observe(rec: &InstanceRecord) -> Probe {
    let pid_alive = rec.pid.map(identity::pid_alive).unwrap_or(false);
    let ident = identity::verify(rec);
    let tcp_ok = pid_alive && probe_tcp(rec);

    let observed = match (rec.state, pid_alive) {
        (InstanceState::Starting, false) => Observed::Failed,
        (InstanceState::Starting, true) => {
            if tcp_ok {
                Observed::Ready
            } else {
                Observed::Starting
            }
        }
        (InstanceState::Ready, false) => Observed::Stopped,
        (InstanceState::Ready, true) => {
            if !ident.is_match() || !tcp_ok {
                Observed::Stale
            } else if rec.desired_state == DesiredState::Stopped {
                Observed::Held
            } else {
                Observed::Ready
            }
        }
        (InstanceState::Failed, _) => Observed::Failed,
        (InstanceState::Stopped, _) => Observed::Stopped,
    };

    Probe {
        observed,
        identity: ident,
        pid_alive,
        tcp_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec_with(state: InstanceState, pid: Option<u32>) -> InstanceRecord {
        let mut r = InstanceRecord::new("t");
        r.state = state;
        r.pid = pid;
        r
    }

    #[test]
    fn starting_without_process_is_failed() {
        let r = rec_with(InstanceState::Starting, None);
        assert_eq!(observe(&r).observed, Observed::Failed);
    }

    #[test]
    fn ready_without_process_is_stopped() {
        let r = rec_with(InstanceState::Ready, None);
        assert_eq!(observe(&r).observed, Observed::Stopped);
    }

    #[test]
    fn ready_with_dead_pid_is_stopped() {
        // 几乎不可能存在的 pid
        let r = rec_with(InstanceState::Ready, Some(u32::MAX));
        assert_eq!(observe(&r).observed, Observed::Stopped);
    }

    #[test]
    fn starting_with_alive_pid_stays_starting_when_port_closed() {
        let mut r = rec_with(InstanceState::Starting, Some(std::process::id()));
        r.port = Some(1); // 无监听
        assert_eq!(observe(&r).observed, Observed::Starting);
    }

    #[test]
    fn ready_with_identity_mismatch_is_stale() {
        // 记录指向自身 pid（非 serve）→ 身份不符 → stale
        let mut r = rec_with(InstanceState::Ready, Some(std::process::id()));
        r.port = Some(1);
        r.boot_id = identity::boot_id();
        r.pid_start_ticks = identity::pid_start_ticks(std::process::id());
        let p = observe(&r);
        assert_eq!(p.observed, Observed::Stale);
        assert!(matches!(p.identity, Identity::Mismatch(_)));
    }

    #[test]
    fn failed_and_stopped_pass_through() {
        assert_eq!(
            observe(&rec_with(InstanceState::Failed, None)).observed,
            Observed::Failed
        );
        assert_eq!(
            observe(&rec_with(InstanceState::Stopped, None)).observed,
            Observed::Stopped
        );
    }

    #[test]
    fn prunable_covers_terminal_states_only() {
        assert!(Observed::Stale.is_prunable());
        assert!(Observed::Stopped.is_prunable());
        assert!(Observed::Failed.is_prunable());
        assert!(!Observed::Ready.is_prunable());
        assert!(!Observed::Starting.is_prunable());
        assert!(!Observed::Held.is_prunable());
    }
}
