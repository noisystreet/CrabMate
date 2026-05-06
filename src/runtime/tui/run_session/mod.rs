//! **阶段 C**：全屏 TUI 内最小对话闭环，复用 [`crate::runtime::cli::repl::repl_dispatch_chat_round`]。
//!
//! 与 REPL 共用配置加载、`CliToolRuntime`、首轮消息准备；**不向 stdout 渲染助手输出**（`suppress_stdout_render`），可按 CLI **`--no-stream`** 选择是否 SSE。
//!
//! **`/` 内建命令**：与 REPL 同源（[`try_handle_repl_slash_command`] + [`repl_slash_handled_followup`]），输出捕获至中区 transcript；**/probe、/models、/mcp** 会短暂退出全屏写 stdout。
//!
//! 架构：专用线程跑 ratatui + crossterm；[`tokio::sync::mpsc::unbounded_channel`] 投递输入；异步侧执行回合并刷新快照。
//!
//! **焦点**：左/中上（聊天）/中下（撰写）/右四块可点击聚焦（**`EnableMouseCapture`**），边框与标题高亮；**`Tab` / `Shift+Tab`** 循环焦点。字符输入与退格仅在 **「撰写」** 聚焦时生效；**`Enter`** 始终提交当前输入行。
//!
//! **工具审批**：全屏居中 Modal（↑↓ / jk · Enter · Esc · 1/2/3），与 REPL dialoguer 三项语义一致；不退出 alternate screen。
//!
//! **撰写区**：按单元格宽度自动换行（宽字符计入 **`unicode-width`**）；纵向往下溢出时仅保留底部可见行（滚动）；**「撰写」** 聚焦时 **`Frame::set_cursor_position`** 显示插入光标。

mod approval;
mod render;

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Stdout, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
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
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Text};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::SharedAgentConfig;
use crate::runtime::cli::{
    CliMainInvocationCommon, ReplAfterUserMessageEnqueuedCb, ReplDispatchChatRoundParams,
    ReplSlashFollowupCtx, ReplSlashHandled, ReplSlashSharedHandles, cli_effective_work_dir,
    repl_dispatch_chat_round, repl_prepare_messages_and_editor, repl_slash_handled_followup,
    try_handle_repl_slash_command,
};
use crate::runtime::cli_exit::{CliExitError, EXIT_USAGE};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::runtime::workspace_session;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::tool_approval::TuiApprovalRequest;
use crate::tool_registry::CliToolRuntime;
use crate::types::{
    Message, is_message_visible_in_chat_transcript, message_content_plain_for_chat_display,
};

/// 撰写区行首提示符（与 [`composer_wrap_lines`] 起始列一致）。
const COMPOSER_PROMPT_PREFIX: &str = "› ";

fn build_tui_nav_summary(
    work_dir: &std::path::Path,
    tui_load_on_start: bool,
    session_file_exists: bool,
    message_count: usize,
) -> String {
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 40);
    let sess = if session_file_exists { "有" } else { "无" };
    let load = if tui_load_on_start { "开" } else { "关" };
    format!(
        "工作区\n{wd_short}\n\n会话文件\ntui_session.json：{sess}\n启动加载：{load}\n\n内存消息\n{message_count} 条（含 system / 工具）\n\n中区仅展示 transcript\n可见尾部",
    )
}

fn build_tui_right_summary(tool_count: usize) -> String {
    format!(
        "侧栏占位\n\n敏感工具审批：全屏 Modal（↑↓ · Enter · Esc · 1/2/3）。\n\n已加载工具：{tool_count} 个",
    )
}

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

/// 与 [`render::render_full`] 一致的分区，供鼠标命中与绘制共用。
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
    /// 左栏：工作区路径、`tui_session.json` 与加载开关等（阶段 D）
    nav_summary: String,
    /// 右栏：任务等占位 + 工具数量提示
    right_summary: String,
    transcript: String,
    /// 聊天区垂直滚动（`Paragraph::scroll` 的 y）；须与 [`render::clamped_chat_vertical_scroll`] 一致地 clamp，避免 ratatui `scroll_y` 过大导致溢出 panic。
    chat_scroll_y: u16,
    input: String,
    status: String,
    focus: TuiFocus,
    /// 敏感工具审批 Modal（单条）；多条时先入队。
    approval_modal: Option<approval::TuiApprovalModalState>,
    approval_backlog: VecDeque<TuiApprovalRequest>,
}

async fn tui_refresh_after_slash_capture(
    model: &Arc<Mutex<TuiModel>>,
    captured: Vec<String>,
    cfg_holder: &SharedAgentConfig,
    work_dir: &std::path::Path,
    message_count: usize,
    tool_count: usize,
    cli_no_stream: bool,
) {
    let new_header = tui_header_summary(cfg_holder, work_dir).await;
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let nav = build_tui_nav_summary(
        work_dir,
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        message_count,
    );
    let right = build_tui_right_summary(tool_count);
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    if !captured.is_empty() {
        g.transcript.push_str("\n[/]\n");
        for ln in captured {
            g.transcript.push_str(&ln);
            g.transcript.push('\n');
        }
        g.transcript.push('\n');
    }
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.status = format!(
        "就绪 · {} 条 · {}",
        message_count,
        status_hint(cli_no_stream)
    );
}

async fn tui_refresh_after_chat_round(
    model: &Arc<Mutex<TuiModel>>,
    cfg_holder: &SharedAgentConfig,
    work_dir: &std::path::Path,
    messages: &[Message],
    tool_count: usize,
    cli_no_stream: bool,
) {
    let new_header = tui_header_summary(cfg_holder, work_dir).await;
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let nav = build_tui_nav_summary(
        work_dir,
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        messages.len(),
    );
    let right = build_tui_right_summary(tool_count);
    let transcript = messages_to_transcript(messages);
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.transcript = transcript;
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.status = format!(
        "就绪 · {} 条消息 · {}",
        messages.len(),
        status_hint(cli_no_stream)
    );
}

/// 进入全屏 TUI 并跑对话循环（须 TTY）。**`cli_no_stream`** 对应全局 **`--no-stream`**；助手正文不因流式写入 stdout（保护 alternate screen）。
pub async fn run_tui_session(
    common: CliMainInvocationCommon<'_>,
    cli_no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let CliMainInvocationCommon {
        cfg_holder,
        config_path,
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
    let (handoff_tx, handoff_rx) = std::sync::mpsc::channel::<TuiTerminalHandoffOp>();
    let (tui_approval_tx, tui_approval_rx) = std::sync::mpsc::sync_channel::<TuiApprovalRequest>(8);
    let cli_rt =
        CliToolRuntime::new_interactive_default().with_tui_blocking_approval(tui_approval_tx);
    let style = CliReplStyle::new();
    let api_key_holder = Arc::new(std::sync::Mutex::new(api_key.to_string()));
    let slash_handles = ReplSlashSharedHandles {
        api_key_holder: Arc::clone(&api_key_holder),
        process_handles: Arc::clone(&process_handles),
    };

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
    let nav_summary = build_tui_nav_summary(
        work_dir.as_path(),
        tui_load,
        workspace_session::session_file_path(work_dir.as_path()).exists(),
        messages.len(),
    );
    let right_summary = build_tui_right_summary(tools.len());

    let llm_scratch: TuiLlmStreamScratchArc = Arc::new(Mutex::new(TuiLlmStreamScratch::default()));

    let (ev_tx, mut ev_rx) = unbounded_channel::<UiEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let model = Arc::new(Mutex::new(TuiModel {
        header_line,
        nav_summary,
        right_summary,
        transcript: messages_to_transcript(&messages),
        chat_scroll_y: 0,
        input: String::new(),
        status: status_hint(cli_no_stream),
        focus: TuiFocus::default(),
        approval_modal: None,
        approval_backlog: VecDeque::new(),
    }));

    let model_th = Arc::clone(&model);
    let scratch_th = Arc::clone(&llm_scratch);
    let shutdown_th = Arc::clone(&shutdown);
    let ui_handle: JoinHandle<io::Result<()>> = std::thread::spawn(move || {
        run_tui_ui_thread(
            model_th,
            scratch_th,
            ev_tx,
            shutdown_th,
            tui_approval_rx,
            handoff_rx,
        )
    });

    while let Some(ev) = ev_rx.recv().await {
        match ev {
            UiEvent::Quit => break,
            UiEvent::Submit(input) => {
                let trimmed = input.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.starts_with('/') {
                    let cap = Arc::new(Mutex::new(Vec::<String>::new()));
                    let style_cap = CliReplStyle::new_tui_capture(Arc::clone(&cap));
                    let handled = try_handle_repl_slash_command(
                        trimmed.as_str(),
                        cfg_holder,
                        tools,
                        &mut messages,
                        &mut work_dir,
                        &style_cap,
                        cli_no_stream,
                        &mut agent_role_owned,
                        &slash_handles,
                    )
                    .await;
                    if matches!(handled, ReplSlashHandled::NotSlash) {
                        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                        g.status =
                            "输入以 / 开头但未识别为内建命令（不应发生）；请报告 issue".to_string();
                        continue;
                    }
                    repl_slash_handled_followup(
                        handled,
                        ReplSlashFollowupCtx {
                            cfg_holder,
                            config_path,
                            client,
                            slash_handles: &slash_handles,
                            style: &style_cap,
                            work_dir: work_dir.as_path(),
                            tui_terminal_tx: Some(&handoff_tx),
                        },
                    )
                    .await?;
                    let captured = cap.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    tui_refresh_after_slash_capture(
                        &model,
                        captured,
                        cfg_holder,
                        work_dir.as_path(),
                        messages.len(),
                        tools.len(),
                        cli_no_stream,
                    )
                    .await;
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
                tui_refresh_after_chat_round(
                    &model,
                    cfg_holder,
                    work_dir.as_path(),
                    messages.as_slice(),
                    tools.len(),
                    cli_no_stream,
                )
                .await;
            }
        }
    }

    if tui_load
        && let Err(e) = workspace_session::save_workspace_session(work_dir.as_path(), &messages)
    {
        eprintln!(
            "写入 {} 失败: {e}",
            workspace_session::session_file_path(work_dir.as_path()).display()
        );
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
        "Enter 发送 · 空行 q · Ctrl+C · /help · Tab 切焦点 · 鼠标点面板 · 聊天区 PgUp/PgDn 滚动",
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

/// UI 线程轮询：`/doctor` 等 stdout 交接与工具审批队列。
struct TuiBlockingRecv<'a> {
    approval_rx: &'a Receiver<TuiApprovalRequest>,
    handoff_rx: &'a Receiver<TuiTerminalHandoffOp>,
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

fn run_tui_ui_thread(
    model: Arc<Mutex<TuiModel>>,
    llm_scratch: TuiLlmStreamScratchArc,
    ev_tx: UnboundedSender<UiEvent>,
    shutdown: Arc<AtomicBool>,
    approval_rx: Receiver<TuiApprovalRequest>,
    handoff_rx: Receiver<TuiTerminalHandoffOp>,
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
    let r = run_tui_poll_loop(
        &mut terminal,
        &model,
        &llm_scratch,
        &ev_tx,
        &shutdown,
        &blocking_recv,
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

enum TuiPollKeyFlow {
    BreakLoop,
    ContinueOuter,
}

fn tui_dispatch_mouse(model: &Arc<Mutex<TuiModel>>, mouse: event::MouseEvent) {
    let modal_open = model
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .approval_modal
        .is_some();
    if modal_open {
        return;
    }
    let Ok((w, h)) = terminal_size() else {
        return;
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

fn tui_dispatch_key_press(
    model: &Arc<Mutex<TuiModel>>,
    ev_tx: &UnboundedSender<UiEvent>,
    key: &event::KeyEvent,
) -> TuiPollKeyFlow {
    match approval::handle_approval_modal_keys(model, ev_tx, key) {
        approval::ApprovalModalKeyOutcome::QuitApp => return TuiPollKeyFlow::BreakLoop,
        approval::ApprovalModalKeyOutcome::Consumed => return TuiPollKeyFlow::ContinueOuter,
        approval::ApprovalModalKeyOutcome::NotApplicable => {}
    }
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            approval::deny_all_pending_approvals(&mut g);
            let _ = ev_tx.send(UiEvent::Quit);
            TuiPollKeyFlow::BreakLoop
        }
        KeyCode::Char(ch @ ('q' | 'Q')) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let input_empty = {
                let g = model.lock().unwrap_or_else(|e| e.into_inner());
                g.input.is_empty()
            };
            if input_empty {
                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                approval::deny_all_pending_approvals(&mut g);
                let _ = ev_tx.send(UiEvent::Quit);
                return TuiPollKeyFlow::BreakLoop;
            }
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            g.input.push(ch);
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::BackTab => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            g.focus = g.focus.cycle_prev();
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::Tab => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            g.focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                g.focus.cycle_prev()
            } else {
                g.focus.cycle_next()
            };
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::PageUp => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            if g.focus == TuiFocus::Chat {
                g.chat_scroll_y = g.chat_scroll_y.saturating_sub(8);
            }
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::PageDown => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            if g.focus == TuiFocus::Chat {
                g.chat_scroll_y = g.chat_scroll_y.saturating_add(8);
            }
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::Enter => {
            let line = {
                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut g.input)
            };
            let _ = ev_tx.send(UiEvent::Submit(line));
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::Backspace => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            if g.focus == TuiFocus::Composer {
                g.input.pop();
            }
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return TuiPollKeyFlow::ContinueOuter;
            }
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            if g.focus == TuiFocus::Composer {
                g.input.push(ch);
            }
            TuiPollKeyFlow::ContinueOuter
        }
        _ => TuiPollKeyFlow::ContinueOuter,
    }
}

fn run_tui_poll_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    model: &Arc<Mutex<TuiModel>>,
    llm_scratch: &TuiLlmStreamScratchArc,
    ev_tx: &UnboundedSender<UiEvent>,
    shutdown: &AtomicBool,
    blocking_recv: &TuiBlockingRecv<'_>,
    color: bool,
) -> io::Result<()> {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        process_tui_main_thread_ops(terminal, blocking_recv, model)?;
        {
            let guard = model.lock().unwrap_or_else(|e| e.into_inner());
            terminal.draw(|frame| render::render_full(frame, &guard, llm_scratch, color))?;
        }

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Mouse(mouse) => tui_dispatch_mouse(model, mouse),
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match tui_dispatch_key_press(model, ev_tx, &key) {
                        TuiPollKeyFlow::BreakLoop => break,
                        TuiPollKeyFlow::ContinueOuter => continue,
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
