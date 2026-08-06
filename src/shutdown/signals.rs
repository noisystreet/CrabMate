//! SIGTERM / SIGINT（Ctrl+C）监听：首次优雅关闭，再次 Ctrl+C 强退。

use super::GracefulShutdown;

const FORCE_EXIT_CODE: i32 = 130;

pub(super) fn spawn(shutdown: GracefulShutdown) {
    tokio::spawn(async move {
        #[cfg(unix)]
        unix_signal_loop(shutdown).await;
        #[cfg(not(unix))]
        ctrl_c_loop(shutdown).await;
    });
}

fn force_exit_repeat_interrupt(label: &str) -> ! {
    log::warn!("再次收到 {label}，立即退出");
    std::process::exit(FORCE_EXIT_CODE);
}

fn on_interrupt(shutdown: &GracefulShutdown, label: &str) {
    if shutdown.is_triggered() {
        force_exit_repeat_interrupt(label);
    }
    log::info!("收到 {label}，开始优雅关闭…（再次 Ctrl+C 将立即退出）");
    shutdown.trigger();
}

#[cfg(unix)]
async fn unix_signal_loop(shutdown: GracefulShutdown) {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = signal(SignalKind::terminate()).expect("无法注册 SIGTERM 处理");
    let mut sigint = signal(SignalKind::interrupt()).expect("无法注册 SIGINT 处理");

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                log::info!("收到 SIGTERM，开始优雅关闭...");
                shutdown.trigger();
                return;
            }
            _ = sigint.recv() => on_interrupt(&shutdown, "SIGINT (Ctrl+C)"),
        }
    }
}

#[cfg(not(unix))]
async fn ctrl_c_loop(shutdown: GracefulShutdown) {
    loop {
        tokio::signal::ctrl_c().await.expect("无法注册 Ctrl+C 处理");
        on_interrupt(&shutdown, "Ctrl+C");
    }
}
