//! 就绪判定：**TCP connect**，不是 `GET /health`。
//!
//! 用 `/health` 做 readiness 有三个问题：该端点会跑外部 CLI 探测（`clang-format`、
//! `cmake`、`gh`…）并可能触发 LLM 端点探测，因此**慢且受外部依赖影响**；配合
//! 「超时就回收进程」会杀掉一个正在正常启动的实例。axum 只在 router 就绪后才
//! `listen`，所以「能 TCP 连上」就是可靠的就绪信号，且不受任何外部依赖干扰。
//!
//! `/health` 仍用于 `ctl status` 的详情展示（它免鉴权，可无 token 直接读）。

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::identity;

/// 等待结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// TCP 已可连接。
    Ready,
    /// 超时未就绪，但**进程仍活着**（不得据此杀进程，见 §readiness 设计）。
    Timeout,
    /// 进程已退出（僵尸或消失）。
    Exited,
}

const CONNECT_ATTEMPT: Duration = Duration::from_millis(300);

/// 轮询直到 TCP 可连、进程退出或超时。
pub fn wait_tcp(addr: SocketAddr, pid: u32, timeout: Duration, poll: Duration) -> WaitOutcome {
    let deadline = Instant::now() + timeout;
    loop {
        if !identity::pid_alive(pid) {
            return WaitOutcome::Exited;
        }
        if TcpStream::connect_timeout(&addr, CONNECT_ATTEMPT).is_ok() {
            return WaitOutcome::Ready;
        }
        if Instant::now() >= deadline {
            // 超时后再确认一次进程状态：崩溃与「慢」必须区分开
            return if identity::pid_alive(pid) {
                WaitOutcome::Timeout
            } else {
                WaitOutcome::Exited
            };
        }
        std::thread::sleep(poll.min(CONNECT_ATTEMPT));
    }
}

/// 解析 `127.0.0.1:8080` 形式的监听地址。
pub fn parse_addr(host: &str, port: u16) -> Result<SocketAddr, super::CtlError> {
    let text = format!("{host}:{port}");
    text.parse::<SocketAddr>().map_err(|e| {
        super::CtlError::usage("bad_addr", format!("无法解析监听地址 {text}：{e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parse_addr_accepts_loopback() {
        let a = parse_addr("127.0.0.1", 8080).expect("应可解析");
        assert_eq!(a.port(), 8080);
    }

    #[test]
    fn parse_addr_rejects_garbage() {
        assert!(parse_addr("not-an-ip", 1).is_err());
    }

    #[test]
    fn detects_ready_when_port_accepting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定随机端口");
        let addr = listener.local_addr().expect("取地址");
        let out = wait_tcp(
            addr,
            std::process::id(),
            Duration::from_secs(2),
            Duration::from_millis(20),
        );
        assert_eq!(out, WaitOutcome::Ready);
    }

    #[test]
    fn reports_exited_when_pid_dead() {
        // 用一个几乎不可能存在的 pid：进程不活时应立刻判 Exited，而不是等满超时
        let addr = parse_addr("127.0.0.1", 9).expect("解析地址");
        let t0 = Instant::now();
        let out = wait_tcp(
            addr,
            u32::MAX,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );
        assert_eq!(out, WaitOutcome::Exited);
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "应快速判死而不是等满超时"
        );
    }

    #[test]
    fn reports_timeout_when_alive_but_port_closed() {
        // 自身进程活着，但目标端口无人监听（9 号 discard 端口通常关闭）
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("占位绑定");
        let closed = listener.local_addr().expect("取地址").port();
        drop(listener);
        let addr = parse_addr("127.0.0.1", closed).expect("解析地址");
        let out = wait_tcp(
            addr,
            std::process::id(),
            Duration::from_millis(150),
            Duration::from_millis(30),
        );
        // 端口刚释放，理论上可能被别的进程占用；只要不是 Exited 即符合语义
        assert_ne!(out, WaitOutcome::Exited, "进程活着不应判 Exited");
    }
}
