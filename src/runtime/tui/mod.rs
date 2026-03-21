//! 终端 UI：左侧对话、右侧工作区/任务/日程；与 Agent 通过 channel + `agent_turn` 协作。

mod agent;
mod allowlist;
mod chat_nav;
mod chat_session;
mod clipboard;
mod draw;
mod edit_history;
mod input;
mod sse_line;
mod state;
mod status;
mod styles;
mod text_input;
mod workspace_ops;

use crate::config::AgentConfig;
use crate::types::Message;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::fs;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use allowlist::{command_approval_message, load_persistent_allowlist};
use chat_session::{load_tui_session, save_tui_session};
use draw::draw_ui;
use input::{HandleKeyContext, handle_crossterm_mouse, handle_key};
use sse_line::{AgentLineKind, classify_agent_sse_line};
use state::{Focus, Mode, ModelPhase, TuiState, strip_sgr_mouse_leaks};
use status::set_normal_status_line;
use workspace_ops::{refresh_schedule, refresh_tasks, refresh_workspace, upsert_assistant_message};

/// 退出全屏 TUI：关鼠标、离开备用屏幕、关 raw，并丢弃 stdin 中残留的鼠标 CSI（避免退出后 shell 上出现 `12;34;56M` 等泄漏）。
fn tui_restore_tty_mouse_and_stdin() -> std::io::Result<()> {
    #[cfg(unix)]
    flush_stdin_tty_queue();

    let mut out = stdout();
    execute!(out, DisableMouseCapture)?;
    execute!(out, LeaveAlternateScreen)?;
    disable_raw_mode()?;

    #[cfg(unix)]
    flush_stdin_tty_queue();
    #[cfg(unix)]
    drain_stdin_nonblocking_best_effort();

    Ok(())
}

#[cfg(unix)]
fn flush_stdin_tty_queue() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    unsafe {
        libc::tcflush(fd, libc::TCIFLUSH);
    }
}

/// 非阻塞读尽 stdin，丢弃已排队但未解析的输入（常见于 Ctrl+C 退出瞬间的 SGR 鼠标序列）。
#[cfg(unix)]
fn drain_stdin_nonblocking_best_effort() {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return;
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return;
    }

    let mut buf = [0u8; 512];
    let mut stdin = std::io::stdin().lock();
    for _ in 0..128 {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }

    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
}

/// 将 CLI/配置中的工作区解析为绝对路径；不存在则创建。必须为目录（不能是普通文件）。
fn resolve_tui_workspace_dir(work_dir_str: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let trimmed = work_dir_str.trim();
    if trimmed.is_empty() {
        return Err("工作区路径不能为空".into());
    }
    let p = PathBuf::from(trimmed);
    if !p.exists() {
        fs::create_dir_all(&p)?;
    }
    let meta = fs::metadata(&p)?;
    if !meta.is_dir() {
        return Err(format!("工作区必须是目录：{}", p.display()).into());
    }
    Ok(fs::canonicalize(&p)?)
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
    let workspace_dir = resolve_tui_workspace_dir(&work_dir_str)?;
    let allowlist_file = workspace_dir
        .join(".crabmate")
        .join("tui_command_allowlist.json");
    let persistent_command_allowlist = load_persistent_allowlist(&allowlist_file);

    let initial_messages = load_tui_session(
        &workspace_dir,
        &cfg.system_prompt,
        cfg.tui_session_max_messages,
    )
    .unwrap_or_else(|| {
        vec![Message {
            role: "system".to_string(),
            content: Some(cfg.system_prompt.clone()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }]
    });

    let mut state = TuiState {
        messages: initial_messages,
        input: String::new(),
        input_cursor: 0,
        prompt: String::new(),
        prompt_cursor: 0,
        prompt_title: String::new(),
        pending_command: String::new(),
        pending_command_args: String::new(),
        pending_approval_allowlist_key: None,
        approve_choice: 0,
        persistent_command_allowlist,
        allowlist_file,
        status_line: String::new(),
        model_phase: ModelPhase::Idle,
        tool_running: false,
        tool_running_clear_pending: false,
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
        show_health: false,
        health_text: String::new(),
        input_rows: 5,
        input_dragging: false,
        input_drag_row: 0,
        chat_first_line: 0,
        chat_follow_tail: true,
        chat_search_matches: Vec::new(),
        chat_search_active_idx: 0,
        pending_focus: None,
        pending_tab: None,
        mouse_leak_scratch: String::new(),
        input_undo: Vec::new(),
        input_redo: Vec::new(),
        prompt_undo: Vec::new(),
        prompt_redo: Vec::new(),
        chat_line_build_cache: Default::default(),
    };
    refresh_workspace(&mut state);
    refresh_tasks(&mut state);
    refresh_schedule(&mut state);
    set_normal_status_line(&mut state, &cfg.model);

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let (tx, mut rx) = mpsc::channel::<String>(2048);
    let (sync_tx, mut sync_rx) = mpsc::channel::<Vec<Message>>(1);
    let mut approval_tx: Option<mpsc::Sender<crate::types::CommandApprovalDecision>> = None;
    let mut agent_running: Option<tokio::task::JoinHandle<()>> = None;
    let agent_cancel = Arc::new(AtomicBool::new(false));
    let mut assistant_buf = String::new();

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();
    // 已离开底部且模型仍在流式输出时，限制重绘频率，减轻 Markdown 每帧重算带来的闪屏。
    let mut last_draw_at = Instant::now();
    let stream_scroll_min_draw_interval = Duration::from_millis(160);

    // 首帧与任意状态变化后为 true；空闲时跳过重绘，避免每 tick 全量重算 Markdown 占满 CPU。
    let mut need_redraw = true;

    loop {
        let mut inbox_changed = false;
        while let Ok(s) = rx.try_recv() {
            inbox_changed = true;
            match classify_agent_sse_line(&s) {
                AgentLineKind::ToolRunning(true) => {
                    state.tool_running = true;
                    state.tool_running_clear_pending = false;
                    state.model_phase = ModelPhase::ToolRunning;
                    set_normal_status_line(&mut state, &cfg.model);
                }
                AgentLineKind::ParsingToolCalls(true) => {
                    state.model_phase = ModelPhase::SelectingTools;
                    set_normal_status_line(&mut state, &cfg.model);
                }
                AgentLineKind::ParsingToolCalls(false) => {
                    if state.model_phase == ModelPhase::SelectingTools {
                        state.model_phase = ModelPhase::Thinking;
                        set_normal_status_line(&mut state, &cfg.model);
                    }
                }
                AgentLineKind::ToolRunning(false) => {
                    // 不在此处立即清掉：否则与 true 同一次 try_recv 排空时，draw 前状态已被还原，用户看不到提示。
                    state.tool_running_clear_pending = true;
                }
                AgentLineKind::WorkspaceRefresh => {
                    refresh_workspace(&mut state);
                    refresh_tasks(&mut state);
                    refresh_schedule(&mut state);
                }
                AgentLineKind::CommandApproval {
                    command,
                    args,
                    allowlist_key,
                } => {
                    state.pending_command = command;
                    state.pending_command_args = args;
                    state.pending_approval_allowlist_key = allowlist_key;
                    state.approve_choice = 0;
                    state.mode = Mode::CommandApprove;
                    state.model_phase = ModelPhase::AwaitingApproval;
                    state.status_line = command_approval_message(
                        &state.pending_command,
                        &state.pending_command_args,
                    );
                }
                AgentLineKind::StreamError => {
                    state.model_phase = ModelPhase::Error;
                    // 不把错误 JSON 写入对话区；底栏左侧阶段词显示为「异常」，右侧保持常规快捷键说明。
                    set_normal_status_line(&mut state, &cfg.model);
                }
                AgentLineKind::Ignore => {}
                AgentLineKind::Plain => {
                    state.model_phase = ModelPhase::Answering;
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
            inbox_changed = true;
            state.messages = msgs;
            assistant_buf.clear();
        }

        if let Some(handle) = agent_running.as_ref()
            && handle.is_finished()
        {
            inbox_changed = true;
            agent_running = None;
            approval_tx = None;
            state.tool_running = false;
            state.tool_running_clear_pending = false;
            state.model_phase = ModelPhase::Idle;
            set_normal_status_line(&mut state, &cfg.model);
        }

        let streaming = agent_running.as_ref().is_some_and(|h| !h.is_finished());
        if state.tool_running_clear_pending {
            state.tool_running_clear_pending = false;
            state.tool_running = false;
            if state.model_phase == ModelPhase::ToolRunning {
                state.model_phase = if streaming {
                    ModelPhase::Thinking
                } else {
                    ModelPhase::Idle
                };
                set_normal_status_line(&mut state, &cfg.model);
            }
            inbox_changed = true;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));
        let screen_size = terminal.size()?;
        let mut had_input = false;
        if event::poll(timeout)? {
            had_input = true;
            match event::read()? {
                Event::Key(key) => {
                    inbox_changed = true;
                    if key.kind != KeyEventKind::Release
                        && handle_key(
                            key,
                            &mut state,
                            HandleKeyContext {
                                agent_running: &mut agent_running,
                                assistant_buf: &mut assistant_buf,
                                approval_tx: &mut approval_tx,
                                tx: &tx,
                                sync_tx: sync_tx.clone(),
                                agent_cancel: agent_cancel.clone(),
                                cfg,
                                client,
                                api_key,
                                tools,
                                no_stream,
                                term_cols: screen_size.width,
                            },
                        )
                        .await?
                    {
                        break;
                    }
                }
                Event::Mouse(m) => {
                    if handle_crossterm_mouse(
                        m,
                        &mut state,
                        screen_size.width,
                        screen_size.height,
                        &cfg.model,
                    ) {
                        inbox_changed = true;
                    }
                }
                Event::Resize(w, h) => {
                    inbox_changed = true;
                    if w > 0 && h > 0 {
                        let _ = terminal.resize(Rect::new(0, 0, w, h));
                    }
                }
                Event::FocusLost | Event::FocusGained => {}
                _ => {}
            }
        }

        let stream_throttled = streaming
            && !state.chat_follow_tail
            && !had_input
            && !state.tool_running
            && state.model_phase != ModelPhase::SelectingTools
            && last_draw_at.elapsed() < stream_scroll_min_draw_interval;

        let should_paint = need_redraw || inbox_changed || (streaming && !stream_throttled);
        need_redraw = false;

        if should_paint {
            terminal.draw(|f| draw_ui(f, &mut state))?;
            last_draw_at = Instant::now();
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    drop(terminal);
    let _ = save_tui_session(&state.workspace_dir, &state.messages);
    let _ = tui_restore_tty_mouse_and_stdin();

    Ok(())
}
