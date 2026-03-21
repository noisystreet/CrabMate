//! 终端 UI：左侧对话、右侧工作区/任务/日程；与 Agent 通过 channel + `agent_turn` 协作。

mod agent;
mod allowlist;
mod draw;
mod input;
mod sse_line;
mod state;
mod status;
mod styles;
mod workspace_ops;

use crate::config::AgentConfig;
use crate::types::Message;
use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use ratatui::termwiz::input::InputEvent;
use ratatui::termwiz::terminal::Terminal as TermwizTerminal;
use ratatui::{backend::TermwizBackend, Terminal};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use allowlist::{command_approval_message, load_persistent_allowlist};
use draw::draw_ui;
use input::{handle_key, handle_mouse, HandleKeyContext};
use sse_line::{classify_agent_sse_line, AgentLineKind};
use state::{strip_sgr_mouse_leaks, Focus, Mode, TuiState};
use status::set_normal_status_line;
use workspace_ops::{refresh_schedule, refresh_tasks, refresh_workspace, upsert_assistant_message};

/// 退出全屏 TUI 前关闭 xterm 鼠标报告并尽量清空 tty 输入队列，避免回到 shell 后出现
/// `51;18;17M` 这类 SGR 鼠标片段被回显在提示符上。
fn tui_restore_tty_mouse_and_stdin(terminal: &mut Terminal<TermwizBackend>) {
    let _ = terminal.backend_mut().buffered_terminal_mut().flush();
    let mut out = std::io::stdout().lock();
    let _ = execute!(out, DisableMouseCapture);
    let _ = std::io::Write::flush(&mut out);
    #[cfg(unix)]
    flush_stdin_tty_queue();
}

#[cfg(unix)]
fn flush_stdin_tty_queue() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    unsafe {
        libc::tcflush(fd, libc::TCIFLUSH);
    }
}

pub async fn run_tui(
    cfg: &AgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    workspace_cli: &Option<String>,
    no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let work_dir_str = workspace_cli
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&cfg.run_command_working_dir)
        .to_string();
    let workspace_dir = std::path::PathBuf::from(work_dir_str);
    let allowlist_file = workspace_dir
        .join(".crabmate")
        .join("tui_command_allowlist.json");
    let persistent_command_allowlist = load_persistent_allowlist(&allowlist_file);

    let mut state = TuiState {
        messages: vec![Message {
            role: "system".to_string(),
            content: Some(cfg.system_prompt.clone()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }],
        input: String::new(),
        prompt: String::new(),
        prompt_title: String::new(),
        pending_command: String::new(),
        pending_command_args: String::new(),
        approve_choice: 0,
        persistent_command_allowlist,
        allowlist_file,
        status_line: String::new(),
        tool_running: false,
        focus: Focus::ChatInput,
        mode: Mode::Normal,
        tab: state::RightTab::Workspace,
        workspace_dir,
        workspace_entries: Vec::new(),
        workspace_sel: 0,
        file_view_title: String::new(),
        file_view_content: String::new(),
        task_items: Vec::new(),
        task_sel: 0,
        reminder_items: Vec::new(),
        reminder_sel: 0,
        event_items: Vec::new(),
        event_sel: 0,
        schedule_sub: 0,
        md_style: 0,
        high_contrast: false,
        code_theme_idx: 0,
        show_help: false,
        input_rows: 5,
        input_dragging: false,
        input_drag_row: 0,
        chat_scroll: 0,
        cursor_override: None,
        cursor_mouse_pos: None,
        pending_focus: None,
        pending_tab: None,
        mouse_leak_scratch: String::new(),
    };
    refresh_workspace(&mut state);
    refresh_tasks(&mut state);
    refresh_schedule(&mut state);
    set_normal_status_line(&mut state, &cfg.model);

    let backend = TermwizBackend::new()?;
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let (tx, mut rx) = mpsc::channel::<String>(2048);
    let (sync_tx, mut sync_rx) = mpsc::channel::<Vec<Message>>(1);
    let mut approval_tx: Option<mpsc::Sender<crate::types::CommandApprovalDecision>> = None;
    let mut agent_running: Option<tokio::task::JoinHandle<()>> = None;
    let mut assistant_buf = String::new();

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        while let Ok(s) = rx.try_recv() {
            match classify_agent_sse_line(&s) {
                AgentLineKind::ToolRunning(true) => {
                    state.tool_running = true;
                    state.status_line = "工具运行中…".to_string();
                }
                AgentLineKind::ToolRunning(false) => {
                    state.tool_running = false;
                    if state.status_line == "工具运行中…" {
                        set_normal_status_line(&mut state, &cfg.model);
                    }
                }
                AgentLineKind::WorkspaceRefresh => {
                    refresh_workspace(&mut state);
                    refresh_tasks(&mut state);
                    refresh_schedule(&mut state);
                }
                AgentLineKind::CommandApproval { command, args } => {
                    state.pending_command = command;
                    state.pending_command_args = args;
                    state.approve_choice = 0;
                    state.mode = Mode::CommandApprove;
                    state.status_line = command_approval_message(
                        &state.pending_command,
                        &state.pending_command_args,
                    );
                }
                AgentLineKind::StreamError => {
                    assistant_buf.push('\n');
                    assistant_buf.push_str(&s);
                    let cleaned = strip_sgr_mouse_leaks(&assistant_buf);
                    if cleaned != assistant_buf {
                        assistant_buf = cleaned;
                    }
                    upsert_assistant_message(&mut state.messages, &assistant_buf);
                }
                AgentLineKind::Ignore => {}
                AgentLineKind::Plain => {
                    assistant_buf.push_str(&s);
                    let cleaned = strip_sgr_mouse_leaks(&assistant_buf);
                    if cleaned != assistant_buf {
                        assistant_buf = cleaned;
                    }
                    upsert_assistant_message(&mut state.messages, &assistant_buf);
                }
            }
        }
        while let Ok(msgs) = sync_rx.try_recv() {
            state.messages = msgs;
            assistant_buf.clear();
        }

        terminal.draw(|f| draw_ui(f, &state))?;
        state.cursor_mouse_pos = None;
        state.cursor_override = None;

        if let Some(handle) = agent_running.as_ref()
            && handle.is_finished()
        {
            agent_running = None;
            approval_tx = None;
            set_normal_status_line(&mut state, &cfg.model);
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));
        let screen_size = terminal.size()?;
        if let Some(input_event) = terminal
            .backend_mut()
            .buffered_terminal_mut()
            .terminal()
            .poll_input(Some(timeout))?
        {
            match input_event {
                InputEvent::Key(key) => {
                    if handle_key(
                        key,
                        &mut state,
                        HandleKeyContext {
                            agent_running: &mut agent_running,
                            assistant_buf: &mut assistant_buf,
                            approval_tx: &mut approval_tx,
                            tx: &tx,
                            sync_tx: sync_tx.clone(),
                            cfg,
                            client,
                            api_key,
                            tools,
                            no_stream,
                        },
                    )
                    .await?
                    {
                        break;
                    }
                }
                InputEvent::Mouse(m) => {
                    handle_mouse(
                        m,
                        &mut state,
                        screen_size.width,
                        screen_size.height,
                        &cfg.model,
                    );
                }
                InputEvent::Resized { .. } => {}
                InputEvent::Paste(_) | InputEvent::Wake | InputEvent::PixelMouse(_) => {}
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    // 在 TermwizBackend Drop 之前显式关鼠标并 flush stdin，减轻退出后 shell 回显鼠标序列。
    tui_restore_tty_mouse_and_stdin(&mut terminal);

    Ok(())
}
