//! 终端 UI：左侧对话、右侧工作区/任务/日程/队列；与 Agent 通过 channel + `agent_turn` 协作。

mod agent;
mod allowlist;
mod chat_nav;
mod clipboard;
mod draw;
mod edit_history;
mod input;
mod state;
mod status;
mod styles;
mod sync_merge;
mod text_input;
mod workspace_ops;

use crate::config::AgentConfig;
use crate::sse::{AgentLineKind, classify_agent_sse_line};
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

use crate::runtime::workspace_session::{initial_workspace_messages, save_workspace_session};
use allowlist::{command_approval_message, load_persistent_allowlist};
use draw::draw_ui;
use input::{HandleKeyContext, handle_crossterm_mouse, handle_key};
use log::debug;
use state::{Focus, Mode, ModelPhase, TuiState, TuiTurnOutcome, strip_sgr_mouse_leaks};
use status::{build_normal_status_line, set_normal_status_line};
use workspace_ops::{refresh_schedule, refresh_tasks, refresh_workspace, upsert_assistant_message};

#[derive(Debug)]
enum TuiAgentEvent {
    StreamLine(String),
    MessagesSnapshot(Vec<Message>),
}

fn coalesce_latest_snapshot(
    snapshot_rx: &mut mpsc::Receiver<Vec<Message>>,
    mut latest: Vec<Message>,
) -> Vec<Message> {
    while let Ok(next) = snapshot_rx.try_recv() {
        latest = next;
    }
    latest
}

async fn forward_pending_snapshots(
    snapshot_open: bool,
    snapshot_rx: &mut mpsc::Receiver<Vec<Message>>,
    event_tx: &mpsc::Sender<TuiAgentEvent>,
) -> bool {
    if !snapshot_open {
        return true;
    }
    while let Ok(msgs) = snapshot_rx.try_recv() {
        let snapshot = coalesce_latest_snapshot(snapshot_rx, msgs);
        if event_tx
            .send(TuiAgentEvent::MessagesSnapshot(snapshot))
            .await
            .is_err()
        {
            return false;
        }
    }
    true
}

fn spawn_tui_event_forwarder(
    mut stream_rx: mpsc::Receiver<String>,
    mut snapshot_rx: mpsc::Receiver<Vec<Message>>,
    event_tx: mpsc::Sender<TuiAgentEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream_open = true;
        let mut snapshot_open = true;
        loop {
            tokio::select! {
                biased;
                stream_item = stream_rx.recv(), if stream_open => {
                    match stream_item {
                        Some(s) => {
                            if event_tx.send(TuiAgentEvent::StreamLine(s)).await.is_err() {
                                return;
                            }
                            // 先把 stream 通道里已到齐的后续行一并发出，再拉快照；避免「只处理一条 SSE 就插入 MessagesSnapshot」打乱时间顺序。
                            while let Ok(next) = stream_rx.try_recv() {
                                if event_tx
                                    .send(TuiAgentEvent::StreamLine(next))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        None => stream_open = false,
                    }
                }
                snapshot_item = snapshot_rx.recv(), if snapshot_open => {
                    match snapshot_item {
                        Some(msgs) => {
                            let snapshot = coalesce_latest_snapshot(&mut snapshot_rx, msgs);
                            if event_tx
                                .send(TuiAgentEvent::MessagesSnapshot(snapshot))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        None => snapshot_open = false,
                    }
                }
                else => break,
            }
            // stream 积压已 drain 完毕（或刚处理完阻塞 recv 的快照）后，立刻下发同刻积压的快照，使分步工具结果不必等整轮 SSE 结束。
            if !forward_pending_snapshots(snapshot_open, &mut snapshot_rx, &event_tx).await {
                return;
            }
            if !stream_open && !snapshot_open {
                break;
            }
        }
    })
}

/// 全量快照合并后重建流式缓冲：若末尾仍是助手正文，继续在其后追加增量；否则清空缓冲。
fn trailing_streaming_assistant_content(messages: &[Message]) -> String {
    messages
        .last()
        .and_then(|m| {
            if m.role == "assistant" && m.tool_calls.is_none() {
                m.content.clone()
            } else {
                None
            }
        })
        .unwrap_or_default()
}

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
    cfg: &std::sync::Arc<AgentConfig>,
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

    let initial_messages =
        initial_workspace_messages(cfg.as_ref(), &workspace_dir, cfg.tui_load_session_on_start);

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
        next_tui_job_id: 0,
        tui_active_job_id: None,
        tui_active_job_started: None,
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
        chat_scroll_min_first_line: 0,
        chat_scroll_max_start: 0,
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
        staged_plan_log: Vec::new(),
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

    let (tx, rx) = mpsc::channel::<String>(2048);
    let (sync_tx, sync_rx) = mpsc::channel::<Vec<Message>>(8);
    let (event_tx, mut event_rx) = mpsc::channel::<TuiAgentEvent>(4096);
    let (turn_outcome_tx, mut turn_outcome_rx) = mpsc::channel::<TuiTurnOutcome>(4);
    let mut approval_tx: Option<mpsc::Sender<crate::types::CommandApprovalDecision>> = None;
    let mut agent_running: Option<tokio::task::JoinHandle<()>> = None;
    let agent_cancel = Arc::new(AtomicBool::new(false));
    let mut assistant_buf = String::new();

    let event_forwarder = spawn_tui_event_forwarder(rx, sync_rx, event_tx);

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();
    // 已离开底部且模型仍在流式输出时，限制重绘频率，减轻 Markdown 每帧重算带来的闪屏。
    let mut last_draw_at = Instant::now();
    let stream_scroll_min_draw_interval = Duration::from_millis(200);

    // 首帧与任意状态变化后为 true；空闲时跳过重绘，避免每 tick 全量重算 Markdown 占满 CPU。
    let mut need_redraw = true;

    loop {
        let mut inbox_changed = false;
        while let Ok(ev) = event_rx.try_recv() {
            inbox_changed = true;
            match ev {
                TuiAgentEvent::StreamLine(s) => match classify_agent_sse_line(&s) {
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
                    AgentLineKind::ToolCall { name, summary } => {
                        state.model_phase = ModelPhase::SelectingTools;
                        let mut msg = String::from("即将执行工具");
                        if let Some(n) = name.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!(" [{}]", n));
                        }
                        if let Some(s) = summary.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!("：{}", s));
                        }
                        state.status_line =
                            format!("{} · {}", msg, build_normal_status_line(&cfg.model));
                    }
                    AgentLineKind::ToolResult {
                        name,
                        summary,
                        ok,
                        exit_code,
                        error_code,
                    } => {
                        let failed = matches!(ok, Some(false))
                            || exit_code.is_some_and(|c| c != 0)
                            || error_code.as_deref().is_some_and(|s| !s.is_empty());
                        let mut msg = if failed {
                            "工具执行失败".to_string()
                        } else {
                            "工具执行完成".to_string()
                        };
                        if let Some(n) = name.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!(" [{}]", n));
                        }
                        if let Some(s) = summary.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!("：{}", s));
                        }
                        if failed {
                            if let Some(c) = error_code.as_deref().filter(|s| !s.is_empty()) {
                                msg.push_str(&format!(" (code={})", c));
                            }
                            if let Some(code) = exit_code {
                                msg.push_str(&format!(" (exit={})", code));
                            }
                        }
                        state.status_line =
                            format!("{} · {}", msg, build_normal_status_line(&cfg.model));
                    }
                    AgentLineKind::StreamError {
                        error_preview,
                        code,
                    } => {
                        state.model_phase = ModelPhase::Error;
                        // 不把错误 JSON 写入对话区；在状态栏保留简要错误信息，便于排障。
                        let mut msg = String::from("流式响应异常");
                        if let Some(c) = code.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!("({})", c));
                        }
                        if let Some(p) = error_preview.as_deref().filter(|s| !s.is_empty()) {
                            msg.push_str(&format!("：{}", p));
                        }
                        state.status_line =
                            format!("{} · {}", msg, build_normal_status_line(&cfg.model));
                    }
                    AgentLineKind::StagedPlanNotice { text, clear_before } => {
                        debug!(
                            target: "crabmate::tui_print",
                            "TUI 分阶段规划通知 clear_before={} text_len={} text={}",
                            clear_before,
                            text.len(),
                            text
                        );
                        const MAX_STAGED_PLAN_LOG: usize = 200;
                        if clear_before {
                            state.staged_plan_log.clear();
                        }
                        for line in text.lines() {
                            let t = line.trim_end();
                            if !t.is_empty()
                                && !crate::runtime::message_display::is_staged_plan_placeholder_like_line(
                                    t,
                                )
                            {
                                state.staged_plan_log.push(t.to_string());
                                while state.staged_plan_log.len() > MAX_STAGED_PLAN_LOG {
                                    state.staged_plan_log.remove(0);
                                }
                            }
                        }
                        let hint =
                            crate::runtime::message_display::staged_plan_notice_status_hint(&text);
                        state.status_line = if hint.is_empty() {
                            build_normal_status_line(&cfg.model)
                        } else {
                            format!("{} · {}", hint, build_normal_status_line(&cfg.model))
                        };
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
                },
                TuiAgentEvent::MessagesSnapshot(msgs) => {
                    let n = msgs.len();
                    let (last_role, last_content) = msgs
                        .last()
                        .map(|m| {
                            (
                                m.role.as_str(),
                                m.content
                                    .as_deref()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| "<empty>".to_string()),
                            )
                        })
                        .unwrap_or(("<none>", String::new()));
                    debug!(
                        target: "crabmate::tui_print",
                        "TUI 会话消息全量同步 count={} last_role={} last_content={}",
                        n,
                        last_role,
                        last_content
                    );
                    // Agent 侧可能已裁剪上下文：直接替换会丢掉较早分步气泡；合并保留前缀再接上尾部。
                    state.messages = sync_merge::merge_tui_messages_after_agent_sync(
                        std::mem::take(&mut state.messages),
                        msgs,
                    );
                    assistant_buf = trailing_streaming_assistant_content(&state.messages);
                }
            }
        }
        while let Ok(o) = turn_outcome_rx.try_recv() {
            inbox_changed = true;
            state.apply_tui_turn_outcome(o);
        }

        if let Some(handle) = agent_running.as_ref()
            && handle.is_finished()
        {
            inbox_changed = true;
            while let Ok(o) = turn_outcome_rx.try_recv() {
                state.apply_tui_turn_outcome(o);
            }
            agent_running = None;
            approval_tx = None;
            state.tool_running = false;
            state.tool_running_clear_pending = false;
            state.model_phase = ModelPhase::Idle;
            set_normal_status_line(&mut state, &cfg.model);
            if state.tui_active_job_id.is_some() {
                state.tui_active_job_id = None;
                state.tui_active_job_started = None;
            }
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
                                turn_outcome_tx: turn_outcome_tx.clone(),
                                agent_cancel: agent_cancel.clone(),
                                cfg,
                                client,
                                api_key,
                                tools,
                                no_stream,
                                term_cols: screen_size.width,
                                term_rows: screen_size.height,
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

    event_forwarder.abort();
    drop(terminal);
    let _ = save_workspace_session(&state.workspace_dir, &state.messages);
    let _ = tui_restore_tty_mouse_and_stdin();

    Ok(())
}

#[cfg(test)]
#[path = "mod/tests.rs"]
mod tests;
