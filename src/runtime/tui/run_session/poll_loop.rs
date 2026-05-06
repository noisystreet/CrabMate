//! UI 线程 ratatui 轮询：stdout 交接、审批队列、澄清问卷 inbox、键盘/鼠标事件。

use std::io::{self, IsTerminal, Stdout, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::UnboundedSender;

use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::tool_approval::TuiApprovalRequest;

use super::{
    TuiClarificationShared, TuiModel, approval, clarify_modal, render, tui_dispatch_key_press,
    tui_dispatch_mouse, tui_use_ansi_color,
};
use crate::runtime::tui::TuiLlmStreamScratchArc;

/// UI 线程轮询：`/doctor` 等 stdout 交接与工具审批队列。
pub(super) struct TuiBlockingRecv<'a> {
    pub(super) approval_rx: &'a Receiver<TuiApprovalRequest>,
    pub(super) handoff_rx: &'a Receiver<TuiTerminalHandoffOp>,
}

/// [`run_tui_poll_loop`] 的引用参数打包（避免形参过多）。
pub(super) struct TuiPollLoopCtx<'a> {
    pub(super) model: &'a Arc<Mutex<TuiModel>>,
    pub(super) llm_scratch: &'a TuiLlmStreamScratchArc,
    pub(super) ev_tx: &'a UnboundedSender<super::UiEvent>,
    pub(super) shutdown: &'a AtomicBool,
    pub(super) blocking_recv: &'a TuiBlockingRecv<'a>,
    pub(super) clarify: &'a TuiClarificationShared,
    pub(super) color: bool,
}

fn process_tui_main_thread_ops(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    blocking: &TuiBlockingRecv<'_>,
    model: &Arc<Mutex<TuiModel>>,
) -> io::Result<()> {
    while let Ok(op) = blocking.handoff_rx.try_recv() {
        match op {
            TuiTerminalHandoffOp::ReleaseForStdout { ack } => {
                disable_raw_mode()?;
                execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                )?;
                terminal.show_cursor()?;
                let _ = ack.send(());
            }
            TuiTerminalHandoffOp::RestoreTui { ack } => {
                enable_raw_mode()?;
                execute!(
                    terminal.backend_mut(),
                    EnterAlternateScreen,
                    EnableMouseCapture
                )?;
                terminal.hide_cursor()?;
                let _ = ack.send(());
            }
        }
    }
    approval::enqueue_tui_approval_requests(model, blocking.approval_rx);
    Ok(())
}

/// UI 线程入口：ratatui 主循环（澄清问卷状态见 [`TuiClarificationShared`]）。
pub(super) fn run_tui_ui_thread(
    model: Arc<Mutex<TuiModel>>,
    llm_scratch: TuiLlmStreamScratchArc,
    ev_tx: UnboundedSender<super::UiEvent>,
    shutdown: Arc<AtomicBool>,
    approval_rx: Receiver<TuiApprovalRequest>,
    handoff_rx: Receiver<TuiTerminalHandoffOp>,
    clarify: TuiClarificationShared,
) -> io::Result<()> {
    let mut stdout_h = stdout();
    if !(stdout_h.is_terminal() && io::stdin().is_terminal()) {
        eprintln!(
            "crabmate tui 需要交互式终端（stdin/stdout 均为 TTY）。\
             管道或非 TTY 环境请使用 crabmate repl 或 crabmate chat。"
        );
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "tui requires a TTY",
        ));
    }

    enable_raw_mode()?;
    execute!(stdout_h, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout_h);
    let mut terminal = Terminal::new(backend)?;
    let color = tui_use_ansi_color();
    let blocking_recv = TuiBlockingRecv {
        approval_rx: &approval_rx,
        handoff_rx: &handoff_rx,
    };
    let poll_ctx = TuiPollLoopCtx {
        model: &model,
        llm_scratch: &llm_scratch,
        ev_tx: &ev_tx,
        shutdown: &shutdown,
        blocking_recv: &blocking_recv,
        clarify: &clarify,
        color,
    };
    let r = run_tui_poll_loop(&mut terminal, &poll_ctx);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    r
}

fn run_tui_poll_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ctx: &TuiPollLoopCtx<'_>,
) -> io::Result<()> {
    loop {
        if ctx.shutdown.load(Ordering::Relaxed) {
            break;
        }
        clarify_modal::poll_clarification_inbox(&ctx.clarify.inbox, ctx.model);
        process_tui_main_thread_ops(terminal, ctx.blocking_recv, ctx.model)?;
        {
            let guard = ctx.model.lock().unwrap_or_else(|e| e.into_inner());
            terminal
                .draw(|frame| render::render_full(frame, &guard, ctx.llm_scratch, ctx.color))?;
        }

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Mouse(mouse) => tui_dispatch_mouse(ctx.model, mouse, ctx.llm_scratch),
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match clarify_modal::handle_clarification_modal_keys(
                        ctx.model,
                        &ctx.clarify.answers_merge,
                        ctx.ev_tx,
                        &key,
                    ) {
                        clarify_modal::ClarificationModalKeyOutcome::NotApplicable => {}
                        clarify_modal::ClarificationModalKeyOutcome::Consumed => continue,
                    }
                    match tui_dispatch_key_press(ctx.model, ctx.ev_tx, &key) {
                        super::TuiPollKeyFlow::BreakLoop => break,
                        super::TuiPollKeyFlow::ContinueOuter => continue,
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
