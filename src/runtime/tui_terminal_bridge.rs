//! 全屏 TUI 与「需直接写 stdout / 读 stdin」的子命令之间的终端交接（释放 alternate screen + raw mode，完成后恢复）。
use std::io::{self, BufRead};
use std::sync::mpsc::Sender;

/// 由 **TUI UI 线程**消费：释放或恢复 ratatui 接管的终端。
pub(crate) enum TuiTerminalHandoffOp {
    ReleaseForStdout { ack: std::sync::mpsc::Sender<()> },
    RestoreTui { ack: std::sync::mpsc::Sender<()> },
}

pub(crate) fn blocking_release_terminal(tx: &Sender<TuiTerminalHandoffOp>) -> io::Result<()> {
    let (ack_tx, ack_rx) = std::sync::mpsc::channel();
    tx.send(TuiTerminalHandoffOp::ReleaseForStdout { ack: ack_tx })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tui thread unavailable"))?;
    ack_rx
        .recv()
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tui release ack"))?;
    Ok(())
}

pub(crate) fn blocking_restore_terminal(tx: &Sender<TuiTerminalHandoffOp>) -> io::Result<()> {
    let (ack_tx, ack_rx) = std::sync::mpsc::channel();
    tx.send(TuiTerminalHandoffOp::RestoreTui { ack: ack_tx })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tui thread unavailable"))?;
    ack_rx
        .recv()
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "tui restore ack"))?;
    Ok(())
}

pub(crate) fn pause_for_return_to_tui() -> io::Result<()> {
    eprintln!("\n按 Enter 返回全屏 TUI…");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(())
}
