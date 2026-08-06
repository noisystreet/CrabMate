//! 优雅关闭协调器：监听 SIGTERM / SIGINT，通知各组件逐步关闭。

mod signals;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

/// 进程级优雅关闭统一协调。
///
/// # 用法
/// 1. 服务启动时 [`GracefulShutdown::new`] 并 `spawn_signal_handler`。
/// 2. 将 `token()` 传递给各需要感知关闭的组件（`ChatJobQueue`、SSE hub 等）。
/// 3. `axum::serve` 通过 `with_graceful_shutdown(shutdown.wait_for_shutdown())` 等待。
///
/// **Ctrl+C**：第一次触发优雅关闭；在关闭完成前再次 **Ctrl+C** 将 [`std::process::exit`]（退出码 130）。
#[derive(Clone)]
pub struct GracefulShutdown {
    pub(super) triggered: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl GracefulShutdown {
    pub fn new() -> Self {
        Self {
            triggered: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 是否已收到首次关闭信号并开始优雅关闭。
    pub fn is_triggered(&self) -> bool {
        self.triggered.load(Ordering::Acquire)
    }

    /// 触发关闭：标记状态并通知所有等待者。
    pub fn trigger(&self) {
        self.triggered.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// 等待关闭信号（用于 `axum::serve` 的 `with_graceful_shutdown`）。
    pub async fn wait_for_shutdown(&self) {
        self.notify.notified().await;
    }

    /// 生成信号监听任务（SIGTERM + SIGINT / Ctrl+C）。
    pub fn spawn_signal_handler(self) {
        signals::spawn(self);
    }
}

impl Default for GracefulShutdown {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::GracefulShutdown;

    #[test]
    fn trigger_sets_triggered_flag() {
        let g = GracefulShutdown::new();
        assert!(!g.is_triggered());
        g.trigger();
        assert!(g.is_triggered());
    }
}
