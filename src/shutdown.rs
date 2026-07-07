//! 优雅关闭协调器：监听 SIGTERM / SIGINT，通知各组件逐步关闭。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

/// 进程级优雅关闭统一协调。
///
/// # 用法
/// 1. 服务启动时 [`GracefulShutdown::new`] 并 `spawn_signal_handler`。
/// 2. 将 `token()` 传递给各需要感知关闭的组件（`ChatJobQueue`、SSE hub 等）。
/// 3. `axum::serve` 通过 `with_graceful_shutdown(shutdown.wait_for_shutdown())` 等待。
#[derive(Clone)]
pub struct GracefulShutdown {
    triggered: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl GracefulShutdown {
    pub fn new() -> Self {
        Self {
            triggered: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 是否已触发关闭。
    #[allow(dead_code)]
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
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigterm = signal(SignalKind::terminate()).expect("无法注册 SIGTERM 处理");
                let mut sigint = signal(SignalKind::interrupt()).expect("无法注册 SIGINT 处理");
                tokio::select! {
                    _ = sigterm.recv() => {
                        log::info!("收到 SIGTERM，开始优雅关闭...");
                    }
                    _ = sigint.recv() => {
                        log::info!("收到 SIGINT，开始优雅关闭...");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.expect("无法注册 Ctrl+C 处理");
                log::info!("收到 Ctrl+C，开始优雅关闭...");
            }
            self.trigger();
        });
    }
}

impl Default for GracefulShutdown {
    fn default() -> Self {
        Self::new()
    }
}
