//! **阶段 C**：全屏 TUI 内最小对话闭环，复用 [`crate::runtime::cli::repl::repl_dispatch_chat_round`]。
//!
//! 与 REPL 共用配置加载、`CliToolRuntime`、首轮消息准备；**不向 stdout 渲染助手输出**（`suppress_stdout_render`），可按 CLI **`--no-stream`** 选择是否 SSE。
//!
//! **`/api-key`**：见 [`crate::runtime::cli::try_dispatch_api_key_slash_for_tui`]，反馈写入中区 transcript。
//!
//! 架构：专用线程跑 ratatui + crossterm；[`tokio::sync::mpsc::unbounded_channel`] 投递输入；异步侧执行回合并刷新快照。
//!
//! **焦点**：左/中上（聊天）/中下（撰写）/右四块可点击聚焦（**`EnableMouseCapture`**），边框与标题高亮；**`Tab` / `Shift+Tab`** 循环焦点。字符输入与退格仅在 **「撰写」** 聚焦时生效；**`Enter`** 始终提交当前输入行。
//!
//! **撰写区**：按单元格宽度自动换行（宽字符计入 **`unicode-width`**）；纵向往下溢出时仅保留底部可见行（滚动）；**「撰写」** 聚焦时 **`Frame::set_cursor_position`** 显示插入光标。

use std::io::{self, IsTerminal, Stdout, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    size as terminal_size,
};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::SharedAgentConfig;
use crate::runtime::cli::repl_parse::classify_repl_slash_command;
use crate::runtime::cli::{
    CliMainInvocationCommon, ReplAfterUserMessageEnqueuedCb, ReplDispatchChatRoundParams,
    cli_effective_work_dir, repl_dispatch_chat_round, repl_prepare_messages_and_editor,
    try_dispatch_api_key_slash_for_tui,
};
use crate::runtime::cli_exit::{CliExitError, EXIT_USAGE};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
use crate::text_util::truncate_chars_with_ellipsis;
use crate::tool_registry::CliToolRuntime;
use crate::types::{
    Message, is_message_visible_in_chat_transcript, message_content_plain_for_chat_display,
};

/// 撰写区行首提示符（与 [`composer_wrap_lines`] 起始列一致）。
const COMPOSER_PROMPT_PREFIX: &str = "› ";

fn tui_use_ansi_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

enum UiEvent {
    Quit,
    Submit(String),
}

/// 可聚焦面板（鼠标点击 / Tab 切换）；用于边框高亮。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum TuiFocus {
    NavLeft,
    Chat,
    #[default]
    Composer,
    SideRight,
}

impl TuiFocus {
    fn cycle_next(self) -> Self {
        match self {
            Self::NavLeft => Self::Chat,
            Self::Chat => Self::Composer,
            Self::Composer => Self::SideRight,
            Self::SideRight => Self::NavLeft,
        }
    }

    fn cycle_prev(self) -> Self {
        match self {
            Self::NavLeft => Self::SideRight,
            Self::Chat => Self::NavLeft,
            Self::Composer => Self::Chat,
            Self::SideRight => Self::Composer,
        }
    }
}

/// 与 [`render_full`] 一致的分区，供鼠标命中与绘制共用。
struct TuiPaneLayout {
    nav_left: Rect,
    chat: Rect,
    composer: Rect,
    side_right: Rect,
}

fn compute_tui_pane_layout(area: Rect) -> TuiPaneLayout {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(23),
            Constraint::Percentage(54),
            Constraint::Percentage(23),
        ])
        .split(vertical[1]);

    // 撰写区固定高度，避免随终端拉高占用过多；聊天区吃掉剩余空间。
    let center_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(4)])
        .split(horizontal[1]);

    TuiPaneLayout {
        nav_left: horizontal[0],
        chat: center_chunks[0],
        composer: center_chunks[1],
        side_right: horizontal[2],
    }
}

fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    let cx = r.x <= col && col < r.x.saturating_add(r.width);
    let cy = r.y <= row && row < r.y.saturating_add(r.height);
    cx && cy
}

fn focus_at_point(layout: &TuiPaneLayout, col: u16, row: u16) -> Option<TuiFocus> {
    if rect_contains(layout.nav_left, col, row) {
        return Some(TuiFocus::NavLeft);
    }
    if rect_contains(layout.chat, col, row) {
        return Some(TuiFocus::Chat);
    }
    if rect_contains(layout.composer, col, row) {
        return Some(TuiFocus::Composer);
    }
    if rect_contains(layout.side_right, col, row) {
        return Some(TuiFocus::SideRight);
    }
    None
}

/// 按显示宽度折行，返回每一行文本及逻辑光标所在行、列（列宽为单元格，`>= max_width` 时表示换行后的「下一行首」）。
fn composer_wrap_lines(max_width: usize, input: &str) -> (Vec<String>, usize, usize) {
    let mut lines = vec![String::from(COMPOSER_PROMPT_PREFIX)];
    let mut row = 0usize;
    let mut col = COMPOSER_PROMPT_PREFIX.width();

    for ch in input.chars() {
        let mut w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w == 0 {
            lines[row].push(ch);
            continue;
        }
        w = w.max(1);
        if col + w > max_width {
            lines.push(String::new());
            row += 1;
            col = 0;
        }
        lines[row].push(ch);
        col += w;
    }

    let mut cur_row = row;
    let mut cur_col = col;
    if cur_col >= max_width {
        cur_row += 1;
        cur_col = 0;
    }
    while lines.len() <= cur_row {
        lines.push(String::new());
    }

    (lines, cur_row, cur_col)
}

/// 生成撰写区可见行（底部对齐滚动）及相对于 inner 左上角的光标坐标。
fn composer_visible_and_cursor_rel(
    inner: Rect,
    input: &str,
) -> (Text<'static>, Option<(u16, u16)>) {
    let mw = inner.width as usize;
    let mh = inner.height as usize;
    if mw == 0 || mh == 0 {
        return (Text::from(Line::from(COMPOSER_PROMPT_PREFIX)), None);
    }
    if COMPOSER_PROMPT_PREFIX.width() > mw {
        let clipped = truncate_chars_with_ellipsis(COMPOSER_PROMPT_PREFIX, mw);
        return (Text::from(Line::from(clipped)), Some((0u16, 0u16)));
    }

    let (lines, cur_row, cur_col) = composer_wrap_lines(mw, input);
    let scroll = lines.len().saturating_sub(mh);
    let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).map(Line::from).collect();
    let cursor_row = cur_row.saturating_sub(scroll);
    let cy = cursor_row.min(mh.saturating_sub(1));
    let cx = cur_col.min(mw.saturating_sub(1));
    (Text::from(visible), Some((cx as u16, cy as u16)))
}

struct TuiModel {
    /// 顶栏一行摘要（对齐 Web 壳层：品牌 · 模型 · 网关 · 工作目录）
    header_line: String,
    transcript: String,
    /// 聊天区垂直滚动（`Paragraph::scroll` 的 y）；须与 [`clamped_chat_vertical_scroll`]  clamp，避免 ratatui `scroll_y` 过大导致溢出 panic。
    chat_scroll_y: u16,
    input: String,
    status: String,
    focus: TuiFocus,
}

fn append_tui_streaming_tail(transcript: &str, scratch: &TuiLlmStreamScratch) -> String {
    let r = scratch.reasoning.trim();
    let c = scratch.content.trim();
    if r.is_empty() && c.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::from(transcript);
    out.push_str("\n────────────────────────────────\n[assistant · 生成中]\n\n");
    if !r.is_empty() {
        out.push_str("(推理) ");
        out.push_str(&truncate_chars_with_ellipsis(r, 8000));
        out.push_str("\n\n");
    }
    if !c.is_empty() {
        out.push_str(&truncate_chars_with_ellipsis(c, 12000));
        out.push('\n');
    }
    out
}

/// 粗算 `Paragraph` + `Wrap` 下的总行数（与 ratatui `WordWrapper` 不完全一致；用于 **限制 scroll_y**，避免 `area.height + scroll_y` 的 `u16` 溢出与 panic）。
fn estimate_wrapped_line_rows(text: &str, inner_width: u16) -> usize {
    let w = inner_width.max(1) as usize;
    if text.is_empty() {
        return 1;
    }
    text.split('\n')
        .map(|line| {
            let lw = UnicodeWidthStr::width(line);
            lw.div_ceil(w).max(1)
        })
        .sum::<usize>()
        .max(1)
}

/// ratatui 0.29：`Paragraph::scroll` 的 `y` 不得大到使内部 `area.height + scroll_y` 溢出；也不得大于「总行数 − 视口行数」。
fn clamped_chat_vertical_scroll(
    text: &str,
    inner_width: u16,
    inner_height: u16,
    stick_to_bottom: bool,
    manual_scroll_y: u16,
) -> u16 {
    let rows = estimate_wrapped_line_rows(text, inner_width);
    let vis = inner_height.max(1) as usize;
    let max_scroll = rows.saturating_sub(vis).min(u16::MAX as usize) as u16;
    if stick_to_bottom {
        max_scroll
    } else {
        manual_scroll_y.min(max_scroll)
    }
}

/// 进入全屏 TUI 并跑对话循环（须 TTY）。**`cli_no_stream`** 对应全局 **`--no-stream`**；助手正文不因流式写入 stdout（保护 alternate screen）。
pub async fn run_tui_session(
    common: CliMainInvocationCommon<'_>,
    cli_no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let CliMainInvocationCommon {
        cfg_holder,
        config_path: _config_path,
        client,
        api_key,
        tools,
        workspace_cli,
        agent_role,
        process_handles,
    } = common;

    let (run_root, tui_load): (String, bool) = {
        let g = cfg_holder.read().await;
        (
            g.command_exec.run_command_working_dir.clone(),
            g.session_ui.tui_load_session_on_start,
        )
    };
    let mut work_dir = cli_effective_work_dir(workspace_cli, &run_root);
    let cli_rt = CliToolRuntime::new_interactive_default();
    let style = CliReplStyle::new();
    let api_key_holder = Arc::new(std::sync::Mutex::new(api_key.to_string()));

    {
        let g = cfg_holder.read().await;
        if let Some(r) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            g.system_prompt_for_new_conversation(Some(r))
                .map_err(|e| CliExitError::new(EXIT_USAGE, e))?;
        }
    }

    let mut agent_role_owned = agent_role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let (mut messages, initial_pending, _repl_editor) = repl_prepare_messages_and_editor(
        cfg_holder,
        tui_load,
        work_dir.as_path(),
        &agent_role_owned,
        run_root.as_str(),
        Arc::clone(&process_handles),
    )
    .await?;

    crate::runtime::workspace_session::try_merge_background_initial_workspace(
        &mut messages,
        initial_pending.as_ref(),
    );

    let header_line = tui_header_summary(cfg_holder, work_dir.as_path()).await;

    let llm_scratch: TuiLlmStreamScratchArc = Arc::new(Mutex::new(TuiLlmStreamScratch::default()));

    let (ev_tx, mut ev_rx) = unbounded_channel::<UiEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let model = Arc::new(Mutex::new(TuiModel {
        header_line,
        transcript: messages_to_transcript(&messages),
        chat_scroll_y: 0,
        input: String::new(),
        status: status_hint(cli_no_stream),
        focus: TuiFocus::default(),
    }));

    let model_th = Arc::clone(&model);
    let scratch_th = Arc::clone(&llm_scratch);
    let shutdown_th = Arc::clone(&shutdown);
    let ui_handle: JoinHandle<io::Result<()>> =
        std::thread::spawn(move || run_tui_ui_thread(model_th, scratch_th, ev_tx, shutdown_th));

    while let Some(ev) = ev_rx.recv().await {
        match ev {
            UiEvent::Quit => break,
            UiEvent::Submit(input) => {
                let trimmed = input.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with('/') {
                    let builtin = classify_repl_slash_command(trimmed.as_str())
                        .expect("slash-prefixed input always classifies to a builtin");
                    if let Some(lines) =
                        try_dispatch_api_key_slash_for_tui(builtin, cfg_holder, &api_key_holder)
                            .await
                    {
                        let new_header = tui_header_summary(cfg_holder, work_dir.as_path()).await;
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        g.transcript.push_str("\n[/]\n");
                        for ln in lines {
                            g.transcript.push_str(&ln);
                            g.transcript.push('\n');
                        }
                        g.transcript.push('\n');
                        g.header_line = new_header;
                        g.status = format!(
                            "就绪 · {} 条 · {}",
                            messages.len(),
                            status_hint(cli_no_stream)
                        );
                        continue;
                    }
                    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                    g.status = "TUI 暂未接入该 / 命令（已支持 /api-key …）；其它请用 crabmate repl"
                        .to_string();
                    continue;
                }
                {
                    let mut s = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
                    s.clear();
                }
                let model_refresh = Arc::clone(&model);
                let cli_ns = cli_no_stream;
                let on_user_enqueued: ReplAfterUserMessageEnqueuedCb =
                    Arc::new(move |msgs: &[Message]| {
                        let t = messages_to_transcript(msgs);
                        let mut g = model_refresh.lock().unwrap_or_else(|e| e.into_inner());
                        g.transcript = t;
                        g.status = format!("生成中 · {} 条 · {}", msgs.len(), status_hint(cli_ns));
                    });
                repl_dispatch_chat_round(ReplDispatchChatRoundParams {
                    input: trimmed,
                    cfg_holder,
                    tools,
                    messages: &mut messages,
                    work_dir: &mut work_dir,
                    style: &style,
                    no_stream: cli_no_stream,
                    suppress_stdout_render: true,
                    tui_llm_stream_scratch: Some(Arc::clone(&llm_scratch)),
                    after_user_message_enqueued: Some(on_user_enqueued),
                    agent_role_owned: &mut agent_role_owned,
                    api_key_holder: &api_key_holder,
                    client,
                    cli_rt: &cli_rt,
                    initial_pending: initial_pending.as_ref(),
                    process_handles: Arc::clone(&process_handles),
                })
                .await?;
                {
                    let mut s = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
                    s.clear();
                }
                let new_header = tui_header_summary(cfg_holder, work_dir.as_path()).await;
                let transcript = messages_to_transcript(&messages);
                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                g.transcript = transcript;
                g.header_line = new_header;
                g.status = format!(
                    "就绪 · {} 条消息 · {}",
                    messages.len(),
                    status_hint(cli_no_stream)
                );
            }
        }
    }

    shutdown.store(true, Ordering::SeqCst);
    let join_out = tokio::task::spawn_blocking(move || ui_handle.join())
        .await
        .map_err(|e| io::Error::other(format!("join tui task: {e:?}")))?
        .map_err(|e| io::Error::other(format!("tui thread join: {e:?}")))?;
    join_out?;

    Ok(())
}

async fn tui_header_summary(cfg_holder: &SharedAgentConfig, work_dir: &std::path::Path) -> String {
    let g = cfg_holder.read().await;
    let model_id = g.llm.model.as_str();
    let base_raw = g.llm.api_base.trim();
    let base = truncate_chars_with_ellipsis(base_raw, 44);
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 52);
    format!("CrabMate · {model_id} · {base} · {wd_short}")
}

fn status_hint(cli_no_stream: bool) -> String {
    let mut s = String::from(
        "Enter 发送 · 空行 q · Ctrl+C · /api-key … · Tab 切焦点 · 鼠标点面板 · 聊天区 PgUp/PgDn 滚动",
    );
    if cli_no_stream {
        s.push_str(" · --no-stream");
    } else {
        s.push_str(" · 流式（不写 stdout）");
    }
    s
}

fn message_body_for_transcript(m: &Message) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(r) = m
        .reasoning_content
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("(推理) {}", truncate_chars_with_ellipsis(r, 2000)));
    }
    let plain = message_content_plain_for_chat_display(&m.content);
    let trimmed = plain.trim();
    if !trimmed.is_empty() {
        parts.push(truncate_chars_with_ellipsis(trimmed, 8000));
    }
    if parts.is_empty() {
        String::new()
    } else {
        parts.join("\n")
    }
}

fn messages_to_transcript(messages: &[Message]) -> String {
    const MAX_TAIL: usize = 48;
    // 与 Web [`filter_messages_for_web_client_snapshot`] 一致：不展示系统提示词与各类注入 user。
    let visible: Vec<&Message> = messages
        .iter()
        .filter(|m| is_message_visible_in_chat_transcript(m))
        .collect();
    let start = visible.len().saturating_sub(MAX_TAIL);
    let mut out = String::new();
    for m in visible.into_iter().skip(start) {
        let body = message_body_for_transcript(m);
        if body.is_empty() {
            continue;
        }
        out.push_str(&format!("[{}]\n{}\n\n", m.role, body));
    }
    const MAX_CHARS: usize = 96_000;
    if out.len() > MAX_CHARS {
        let drain = out.len() - MAX_CHARS;
        let safe = next_char_boundary(&out, drain);
        out.drain(..safe);
    }
    out
}

fn next_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut i = byte_idx.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn run_tui_ui_thread(
    model: Arc<Mutex<TuiModel>>,
    llm_scratch: TuiLlmStreamScratchArc,
    ev_tx: UnboundedSender<UiEvent>,
    shutdown: Arc<AtomicBool>,
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
    let r = run_tui_poll_loop(
        &mut terminal,
        &model,
        &llm_scratch,
        &ev_tx,
        &shutdown,
        color,
    );

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
    model: &Arc<Mutex<TuiModel>>,
    llm_scratch: &TuiLlmStreamScratchArc,
    ev_tx: &UnboundedSender<UiEvent>,
    shutdown: &AtomicBool,
    color: bool,
) -> io::Result<()> {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        {
            let guard = model.lock().unwrap_or_else(|e| e.into_inner());
            terminal.draw(|frame| render_full(frame, &guard, llm_scratch, color))?;
        }

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Mouse(mouse) => {
                    let Ok((w, h)) = terminal_size() else {
                        continue;
                    };
                    let layout = compute_tui_pane_layout(Rect::new(0, 0, w, h));
                    match mouse.kind {
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                            if rect_contains(layout.chat, mouse.column, mouse.row) =>
                        {
                            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                            g.focus = TuiFocus::Chat;
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    g.chat_scroll_y = g.chat_scroll_y.saturating_sub(3);
                                }
                                MouseEventKind::ScrollDown => {
                                    g.chat_scroll_y = g.chat_scroll_y.saturating_add(3);
                                }
                                _ => {}
                            }
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(f) = focus_at_point(&layout, mouse.column, mouse.row) {
                                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                                g.focus = f;
                            }
                        }
                        _ => {}
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let _ = ev_tx.send(UiEvent::Quit);
                        break;
                    }
                    KeyCode::Char(ch @ ('q' | 'Q'))
                        if !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        let input_empty = {
                            let g = model.lock().unwrap_or_else(|e| e.into_inner());
                            g.input.is_empty()
                        };
                        if input_empty {
                            let _ = ev_tx.send(UiEvent::Quit);
                            break;
                        }
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        g.input.push(ch);
                    }
                    KeyCode::BackTab => {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        g.focus = g.focus.cycle_prev();
                    }
                    KeyCode::Tab => {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        g.focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                            g.focus.cycle_prev()
                        } else {
                            g.focus.cycle_next()
                        };
                    }
                    KeyCode::PageUp => {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        if g.focus == TuiFocus::Chat {
                            g.chat_scroll_y = g.chat_scroll_y.saturating_sub(8);
                        }
                    }
                    KeyCode::PageDown => {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        if g.focus == TuiFocus::Chat {
                            g.chat_scroll_y = g.chat_scroll_y.saturating_add(8);
                        }
                    }
                    KeyCode::Enter => {
                        let line = {
                            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                            std::mem::take(&mut g.input)
                        };
                        let _ = ev_tx.send(UiEvent::Submit(line));
                    }
                    KeyCode::Backspace => {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        if g.focus == TuiFocus::Composer {
                            g.input.pop();
                        }
                    }
                    KeyCode::Char(ch) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            continue;
                        }
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        if g.focus == TuiFocus::Composer {
                            g.input.push(ch);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
    Ok(())
}

fn render_full(
    frame: &mut Frame<'_>,
    model: &TuiModel,
    llm_scratch: &TuiLlmStreamScratchArc,
    color: bool,
) {
    let area = frame.area();
    // 对齐 Web `shell-ds`：顶栏 + 三列（侧栏宽≈ nav-rail）+ 底栏快捷键
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    render_top_bar(frame, vertical[0], model.header_line.as_str(), color);

    let panes = compute_tui_pane_layout(area);

    render_side_panel(
        frame,
        panes.nav_left,
        " 导航 · 会话 ",
        "对齐 Web 左侧导航栏。\n· 会话列表（阶段 D）\n· 工作区树与路径",
        color,
        model.focus == TuiFocus::NavLeft,
    );

    // 「撰写」块含四边边框 + 标题，高度若仅 1 行会导致内层为 0。
    let scratch_guard = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
    let streaming_nonempty =
        !scratch_guard.reasoning.trim().is_empty() || !scratch_guard.content.trim().is_empty();
    let chat_body = append_tui_streaming_tail(model.transcript.as_str(), &scratch_guard);
    drop(scratch_guard);
    let chat_block = panel_block(" 聊天 ", color, model.focus == TuiFocus::Chat);
    let chat_inner = chat_block.inner(panes.chat);
    let chat_scroll_y = clamped_chat_vertical_scroll(
        chat_body.as_str(),
        chat_inner.width.max(1),
        chat_inner.height.max(1),
        streaming_nonempty,
        model.chat_scroll_y,
    );
    let center_body = Paragraph::new(chat_body)
        .wrap(Wrap { trim: false })
        .scroll((chat_scroll_y, 0))
        .block(chat_block);
    frame.render_widget(center_body, panes.chat);

    let composer_block = panel_block(" 撰写 ", color, model.focus == TuiFocus::Composer);
    let composer_inner = composer_block.inner(panes.composer);
    let (composer_text, cursor_rel) =
        composer_visible_and_cursor_rel(composer_inner, model.input.as_str());
    let composer_style = if color && model.focus == TuiFocus::Composer {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input_par = Paragraph::new(composer_text)
        .style(composer_style)
        .block(composer_block);
    frame.render_widget(input_par, panes.composer);
    if model.focus == TuiFocus::Composer
        && let Some((cx, cy)) = cursor_rel
    {
        frame.set_cursor_position(Position::new(
            composer_inner.x.saturating_add(cx),
            composer_inner.y.saturating_add(cy),
        ));
    }

    render_side_panel(
        frame,
        panes.side_right,
        " 侧栏 · 任务 ",
        "任务 / 规划时间线（占位）\n上下文摘要 · 调试（占位）\n对齐 Web 右侧栏",
        color,
        model.focus == TuiFocus::SideRight,
    );

    let status_style = status_line_style(color);
    let status_block = Block::default().style(status_style);
    let status_line = if color {
        Line::from(vec![
            Span::styled(model.status.as_str(), Style::default().fg(Color::White)),
            Span::styled(
                " · 配置见 crabmate repl /config",
                Style::default().fg(Color::Gray),
            ),
        ])
    } else {
        Line::from(model.status.as_str())
    };
    let status = Paragraph::new(status_line).block(status_block);
    frame.render_widget(status, vertical[2]);
}

fn render_top_bar(frame: &mut Frame<'_>, area: Rect, header: &str, color: bool) {
    let max_w = area.width.saturating_sub(2).max(4) as usize;
    let text = truncate_chars_with_ellipsis(header, max_w);
    let fg = if color {
        Color::Rgb(200, 204, 212)
    } else {
        Color::Reset
    };
    let bg = if color {
        Color::Rgb(40, 44, 52)
    } else {
        Color::Reset
    };
    let line = Line::from(Span::styled(text, Style::default().fg(fg).bg(bg)));
    let block_style = if color {
        Style::default().bg(bg)
    } else {
        Style::default()
    };
    let p = Paragraph::new(line).block(Block::default().style(block_style));
    frame.render_widget(p, area);
}

fn panel_block(title: &str, color: bool, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(title))
        .title_style(title_style(color, focused))
        .border_style(panel_border_style(color, focused))
}

fn panel_border_style(color: bool, focused: bool) -> Style {
    if color {
        if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn render_side_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    body: &str,
    color: bool,
    focused: bool,
) {
    let paragraph = Paragraph::new(body)
        .wrap(Wrap { trim: true })
        .block(panel_block(title, color, focused));
    frame.render_widget(paragraph, area);
}

fn title_style(color: bool, focused: bool) -> Style {
    if color {
        if focused {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::Cyan)
        }
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn status_line_style(color: bool) -> Style {
    if color {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default()
    }
}
