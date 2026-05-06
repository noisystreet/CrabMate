//! **阶段 C**：全屏 TUI 内最小对话闭环，复用 [`crate::runtime::cli::repl::repl_dispatch_chat_round`]。
//!
//! 与 REPL 共用配置加载、`CliToolRuntime`、首轮消息准备；**不向 stdout 渲染助手输出**（`suppress_stdout_render`），可按 CLI **`--no-stream`** 选择是否 SSE。
//!
//! **`/` 内建命令**：与 REPL 同源（[`try_handle_repl_slash_command`] + [`repl_slash_handled_followup`]），输出捕获至中区 transcript；**/probe、/models、/mcp** 会短暂退出全屏写 stdout。
//!
//! 架构：专用线程跑 ratatui + crossterm；[`tokio::sync::mpsc::unbounded_channel`] 投递输入；异步侧执行回合并刷新快照。
//!
//! **焦点**：左（会话）/中上（聊天）/中下（撰写）/右（工作区）四块可点击聚焦（**`EnableMouseCapture`**），边框与标题高亮；**`Tab` / `Shift+Tab`** 循环焦点。**右侧工作区栏聚焦时 `Enter`** 打开工作区 Modal（与 Web 侧栏一致）；**撰写区聚焦时 `Enter`** 提交输入行。字符输入与退格仅在 **「撰写」** 聚焦时生效；
//!
//! **中区 transcript**：与 Web 快照一致的过滤（[`is_message_visible_in_chat_transcript`]）；**工具**条走 [`crate::runtime::message_display::tool_content_for_display_for_message`]（摘要优先，非原始 JSON）；**助手**走 [`assistant_markdown_source_for_message`]；**用户**走 [`user_message_for_chat_display`]（隐藏分步注入等）。
//!
//! **工具审批**：全屏居中 Modal（↑↓ / jk · Enter · Esc · 1/2/3），与 REPL dialoguer 三项语义一致；不退出 alternate screen。
//!
//! **撰写区**：按单元格宽度自动换行（宽字符计入 **`unicode-width`**）；纵向往下溢出时仅保留底部可见行（滚动）；**「撰写」** 聚焦时 **`Frame::set_cursor_position`** 显示插入光标。
//!
//! **底栏**：对齐 Web `status_bar_footer_view` — **模型 · … · base_url · … · 角色 · … ·** 运行态（**就绪** / **模型生成中…** / **工具执行中…** / **错误:** …）；快捷键说明在右侧栏。
//!
//! **聊天区**：内容溢出时右侧显示滚动条；可 **左键拖动** 滚动条改变纵向位置（与滚轮 / PgUp/PgDn 共用 [`TuiModel::chat_scroll_y`]）。

mod approval;
mod clarify_modal;
mod poll_loop;
mod render;
mod submit_ev;
mod transcript;
mod workspace_modal;
mod workspace_switch;

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crossterm::event::{self, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::size as terminal_size;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Text};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::{AgentConfig, SharedAgentConfig};
use crate::runtime::cli::{
    CliMainInvocationCommon, ReplAfterUserMessageEnqueuedCb, ReplSlashFollowupCtx,
    ReplSlashHandled, ReplSlashSharedHandles, cli_effective_work_dir,
    repl_prepare_messages_and_editor, repl_slash_handled_followup, try_handle_repl_slash_command,
};
use crate::runtime::cli_exit::{CliExitError, EXIT_USAGE};
use crate::runtime::cli_repl_ui::CliReplStyle;
use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
use crate::runtime::tui_terminal_bridge::TuiTerminalHandoffOp;
use crate::runtime::workspace_session;
use crate::text_util::truncate_chars_with_ellipsis;
use crate::tool_approval::TuiApprovalRequest;
use crate::tool_registry::CliToolRuntime;
use crate::types::Message;

/// 撰写区行首提示符（与 [`composer_wrap_lines`] 起始列一致）。
const COMPOSER_PROMPT_PREFIX: &str = "› ";

/// Agent 异步侧与 UI 线程共享：澄清问卷 inbox + 待并入下一条用户消息的答案。
#[derive(Clone)]
pub(super) struct TuiClarificationShared {
    inbox: Arc<Mutex<VecDeque<crate::sse::ClarificationQuestionnaireBody>>>,
    answers_merge: Arc<Mutex<Option<crate::clarification_questionnaire::ClarifyAnswersNormalized>>>,
}

/// 左侧会话栏（对齐 Web：会话在左）。
fn build_tui_session_sidebar(
    tui_load_on_start: bool,
    session_file_exists: bool,
    message_count: usize,
) -> String {
    let sess = if session_file_exists { "有" } else { "无" };
    let load = if tui_load_on_start { "开" } else { "关" };
    format!(
        "会话\n\n会话文件\ntui_session.json：{sess}\n启动加载：{load}\n\n内存消息\n{message_count} 条（含 system / 工具）\n\n中区仅展示 transcript\n可见尾部",
    )
}

/// 右侧工作区栏 + 任务提示（对齐 Web：工作区在右）。
fn build_tui_workspace_sidebar(
    work_dir: &std::path::Path,
    tool_count: usize,
    cli_no_stream: bool,
) -> String {
    let wd = work_dir.display().to_string();
    let wd_short = truncate_chars_with_ellipsis(&wd, 40);
    format!(
        "工作区\n{wd_short}\n\n聚焦本栏按 Enter：浏览/编辑路径\n（与 Web 侧栏工作区、REPL /workspace 同源校验）\n\n快捷键\n{}\n\n敏感工具审批：全屏 Modal（↑↓ · Enter · Esc · 1/2/3）。\n\n已加载工具：{tool_count} 个",
        tui_keyboard_help_compact(cli_no_stream),
    )
}

/// 原底栏文案迁至侧栏；与 `--no-stream` 对齐 REPL 提示。
fn tui_keyboard_help_compact(cli_no_stream: bool) -> String {
    let mut s = String::from(
        "Enter 发送 · 空行 q · Ctrl+C · /help · Tab 切焦点 · 鼠标点面板 · 聊天区 PgUp/PgDn · 右侧滚动条拖动",
    );
    if cli_no_stream {
        s.push_str(" · --no-stream");
    } else {
        s.push_str(" · 流式（不写 stdout）");
    }
    s
}

/// 与 Web 底栏「角色」下拉一致：显式 `/agent set` 显示 id；否则 default / default (配置 id)。
fn tui_status_role_label(agent_role_owned: &Option<String>, cfg: &AgentConfig) -> String {
    if let Some(id) = agent_role_owned
        .as_ref()
        .map(|x| x.trim())
        .filter(|s| !s.is_empty())
    {
        return id.to_string();
    }
    match cfg
        .roles_prompts
        .default_agent_role_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => format!("default ({id})"),
        None => "default".to_string(),
    }
}

/// Web 底栏 chips 段（不含末尾「就绪 / 模型生成中…」等运行态）。
async fn tui_status_chips_line(
    cfg_holder: &SharedAgentConfig,
    agent_role_owned: &Option<String>,
) -> String {
    let g = cfg_holder.read().await;
    let model_id = g.llm.model.as_str();
    let base = truncate_chars_with_ellipsis(g.llm.api_base.trim(), 44);
    let role = tui_status_role_label(agent_role_owned, &g);
    format!("模型 · {model_id} · base_url · {base} · 角色 · {role}")
}

fn tui_status_bar_with_run(chips: &str, run: &str) -> String {
    format!("{chips} · {run}")
}

/// Web `status_model_running` 文案 + TUI 补充的消息条数。
fn tui_status_suffix_model_busy_lines(msg_len: usize) -> String {
    format!("模型生成中… · {msg_len} 条")
}

fn tui_use_ansi_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

enum UiEvent {
    Quit,
    Submit(String),
    /// 工作区路径原始输入（由 Modal 或后续扩展提交）。
    WorkspaceSwitch(String),
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
    /// 左栏：会话文件、`tui_session.json` 与加载开关等（对齐 Web 左侧会话）
    nav_summary: String,
    /// 右栏：工作区路径 + 快捷键 / 工具提示（对齐 Web 右侧工作区）
    right_summary: String,
    transcript: String,
    /// 聊天区垂直滚动（`Paragraph::scroll` 的 y）；须与 [`render::clamped_chat_vertical_scroll`] 一致地 clamp，避免 ratatui `scroll_y` 过大导致溢出 panic。
    chat_scroll_y: u16,
    /// 左键在聊天区纵向滚动条上按下后拖动（[`tui_dispatch_mouse`]）。
    chat_scrollbar_dragging: bool,
    input: String,
    /// 与 Web 底栏 chips 同源快照（`模型 · … · base_url · … · 角色 · …`），供同步回调拼接运行态。
    status_chips: String,
    status: String,
    focus: TuiFocus,
    /// 敏感工具审批 Modal（单条）；多条时先入队。
    approval_modal: Option<approval::TuiApprovalModalState>,
    approval_backlog: VecDeque<TuiApprovalRequest>,
    /// 澄清问卷（与 Web SSE `clarification_questionnaire` 对齐）。
    clarification_modal: Option<clarify_modal::TuiClarificationModalState>,
    clarification_backlog: VecDeque<crate::sse::ClarificationQuestionnaireBody>,
    /// 与异步侧 `work_dir` 同步，供 UI 打开工作区 Modal。
    workspace_path_buf: std::path::PathBuf,
    /// 工作区切换（目录浏览 + 手动路径，对齐 Web `POST /workspace` / REPL `/workspace`）。
    workspace_modal: Option<workspace_modal::TuiWorkspaceModalState>,
}

struct TuiSlashUiRefresh<'a> {
    model: &'a Arc<Mutex<TuiModel>>,
    cfg_holder: &'a SharedAgentConfig,
    work_dir: &'a std::path::Path,
    agent_role_owned: &'a Option<String>,
    message_count: usize,
    tool_count: usize,
    cli_no_stream: bool,
    captured: Vec<String>,
}

pub(super) struct TuiSlashSubmit<'a> {
    cfg_holder: &'a SharedAgentConfig,
    config_path: Option<&'a str>,
    client: &'a reqwest::Client,
    tools: &'a [crate::types::Tool],
    messages: &'a mut Vec<Message>,
    work_dir: &'a mut std::path::PathBuf,
    cli_no_stream: bool,
    agent_role_owned: &'a mut Option<String>,
    slash_handles: &'a ReplSlashSharedHandles,
    model: &'a Arc<Mutex<TuiModel>>,
    handoff_tx: &'a std::sync::mpsc::Sender<TuiTerminalHandoffOp>,
}

pub(super) async fn tui_try_consume_slash_submit(
    trimmed: &str,
    ctx: TuiSlashSubmit<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !trimmed.starts_with('/') {
        return Ok(false);
    }
    let cap = Arc::new(Mutex::new(Vec::<String>::new()));
    let style_cap = CliReplStyle::new_tui_capture(Arc::clone(&cap));
    let handled = try_handle_repl_slash_command(
        trimmed,
        ctx.cfg_holder,
        ctx.tools,
        ctx.messages,
        ctx.work_dir,
        &style_cap,
        ctx.cli_no_stream,
        ctx.agent_role_owned,
        ctx.slash_handles,
    )
    .await;
    if matches!(handled, ReplSlashHandled::NotSlash) {
        let mut g = ctx.model.lock().unwrap_or_else(|e| e.into_inner());
        let chips = g.status_chips.clone();
        g.status = format!(
            "{} · 错误: {}",
            chips, "输入以 / 开头但未识别为内建命令（不应发生）；请报告 issue"
        );
        return Ok(true);
    }
    repl_slash_handled_followup(
        handled,
        ReplSlashFollowupCtx {
            cfg_holder: ctx.cfg_holder,
            config_path: ctx.config_path,
            client: ctx.client,
            slash_handles: ctx.slash_handles,
            style: &style_cap,
            work_dir: ctx.work_dir.as_path(),
            tui_terminal_tx: Some(ctx.handoff_tx),
        },
    )
    .await?;
    let captured = cap.lock().unwrap_or_else(|e| e.into_inner()).clone();
    tui_refresh_after_slash_capture(TuiSlashUiRefresh {
        model: ctx.model,
        cfg_holder: ctx.cfg_holder,
        work_dir: ctx.work_dir.as_path(),
        agent_role_owned: ctx.agent_role_owned,
        message_count: ctx.messages.len(),
        tool_count: ctx.tools.len(),
        cli_no_stream: ctx.cli_no_stream,
        captured,
    })
    .await;
    Ok(true)
}

async fn tui_refresh_after_slash_capture(p: TuiSlashUiRefresh<'_>) {
    let TuiSlashUiRefresh {
        model,
        cfg_holder,
        work_dir,
        agent_role_owned,
        message_count,
        tool_count,
        cli_no_stream,
        captured,
    } = p;
    let new_header = tui_header_summary(cfg_holder, work_dir).await;
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let nav = build_tui_session_sidebar(
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        message_count,
    );
    let right = build_tui_workspace_sidebar(work_dir, tool_count, cli_no_stream);
    let chips = tui_status_chips_line(cfg_holder, agent_role_owned).await;
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
    g.workspace_path_buf = work_dir.to_path_buf();
    g.status_chips = chips.clone();
    g.status = tui_status_bar_with_run(&chips, "就绪");
}

async fn tui_refresh_after_chat_round(
    model: &Arc<Mutex<TuiModel>>,
    cfg_holder: &SharedAgentConfig,
    work_dir: &std::path::Path,
    agent_role_owned: &Option<String>,
    messages: &[Message],
    tool_count: usize,
    cli_no_stream: bool,
) {
    let new_header = tui_header_summary(cfg_holder, work_dir).await;
    let tui_load_nav = cfg_holder.read().await.session_ui.tui_load_session_on_start;
    let nav = build_tui_session_sidebar(
        tui_load_nav,
        workspace_session::session_file_path(work_dir).exists(),
        messages.len(),
    );
    let right = build_tui_workspace_sidebar(work_dir, tool_count, cli_no_stream);
    let chips = tui_status_chips_line(cfg_holder, agent_role_owned).await;
    let transcript = transcript::messages_to_transcript(messages);
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.transcript = transcript;
    g.header_line = new_header;
    g.nav_summary = nav;
    g.right_summary = right;
    g.workspace_path_buf = work_dir.to_path_buf();
    g.status_chips = chips.clone();
    g.status = tui_status_bar_with_run(&chips, "就绪");
}

/// 进入全屏 TUI 并跑对话循环（须 TTY）。**`cli_no_stream`** 对应全局 **`--no-stream`**；助手正文不因流式写入 stdout（保护 alternate screen）。
fn tui_make_submit_hooks(
    model: &Arc<Mutex<TuiModel>>,
) -> (
    ReplAfterUserMessageEnqueuedCb,
    Arc<dyn Fn(bool) + Send + Sync>,
) {
    let msg_len_turn = Arc::new(AtomicUsize::new(0));
    let msg_len_for_cb = Arc::clone(&msg_len_turn);
    let model_refresh = Arc::clone(model);
    let on_user_enqueued: ReplAfterUserMessageEnqueuedCb = Arc::new(move |msgs: &[Message]| {
        msg_len_for_cb.store(msgs.len(), Ordering::SeqCst);
        let t = transcript::messages_to_transcript(msgs);
        let mut g = model_refresh.lock().unwrap_or_else(|e| e.into_inner());
        g.transcript = t;
        let chips = g.status_chips.clone();
        let suf = tui_status_suffix_model_busy_lines(msgs.len());
        g.status = tui_status_bar_with_run(&chips, suf.as_str());
    });
    let model_for_hook = Arc::clone(model);
    let msg_len_for_hook = Arc::clone(&msg_len_turn);
    let tool_running_hook: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |running| {
        let mut g = model_for_hook.lock().unwrap_or_else(|e| e.into_inner());
        let chips = g.status_chips.clone();
        let n = msg_len_for_hook.load(Ordering::SeqCst);
        g.status = if running {
            tui_status_bar_with_run(&chips, "工具执行中…")
        } else {
            let suf = tui_status_suffix_model_busy_lines(n.max(1));
            tui_status_bar_with_run(&chips, suf.as_str())
        };
    });
    (on_user_enqueued, tool_running_hook)
}

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
    let nav_summary = build_tui_session_sidebar(
        tui_load,
        workspace_session::session_file_path(work_dir.as_path()).exists(),
        messages.len(),
    );
    let right_summary = build_tui_workspace_sidebar(work_dir.as_path(), tools.len(), cli_no_stream);
    let status_chips = tui_status_chips_line(cfg_holder, &agent_role_owned).await;
    let status_line = tui_status_bar_with_run(&status_chips, "就绪");

    let llm_scratch: TuiLlmStreamScratchArc = Arc::new(Mutex::new(TuiLlmStreamScratch::default()));

    let (ev_tx, mut ev_rx) = unbounded_channel::<UiEvent>();
    let shutdown = Arc::new(AtomicBool::new(false));
    let model = Arc::new(Mutex::new(TuiModel {
        header_line,
        nav_summary,
        right_summary,
        transcript: transcript::messages_to_transcript(&messages),
        chat_scroll_y: 0,
        chat_scrollbar_dragging: false,
        input: String::new(),
        status_chips,
        status: status_line,
        focus: TuiFocus::default(),
        approval_modal: None,
        approval_backlog: VecDeque::new(),
        clarification_modal: None,
        clarification_backlog: VecDeque::new(),
        workspace_path_buf: work_dir.clone(),
        workspace_modal: None,
    }));

    let clarify_shared = TuiClarificationShared {
        inbox: Arc::new(Mutex::new(VecDeque::<
            crate::sse::ClarificationQuestionnaireBody,
        >::new())),
        answers_merge: Arc::new(Mutex::new(
            None::<crate::clarification_questionnaire::ClarifyAnswersNormalized>,
        )),
    };
    let inbox_hook = Arc::clone(&clarify_shared.inbox);
    let model_hook = Arc::clone(&model);
    let clarification_questionnaire_hook: Arc<
        dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync,
    > = Arc::new(move |body| {
        clarify_modal::enqueue_clarification_from_hook(&inbox_hook, &model_hook, body);
    });

    let model_th = Arc::clone(&model);
    let scratch_th = Arc::clone(&llm_scratch);
    let shutdown_th = Arc::clone(&shutdown);
    let clarify_th = clarify_shared.clone();
    let ui_handle: JoinHandle<io::Result<()>> = std::thread::spawn(move || {
        poll_loop::run_tui_ui_thread(
            model_th,
            scratch_th,
            ev_tx,
            shutdown_th,
            tui_approval_rx,
            handoff_rx,
            clarify_th,
        )
    });

    while let Some(ev) = ev_rx.recv().await {
        match ev {
            UiEvent::Quit => break,
            UiEvent::Submit(input) => {
                let trimmed = input.trim().to_string();
                let allow_empty = clarify_shared
                    .answers_merge
                    .lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false);
                if trimmed.is_empty() && !allow_empty {
                    continue;
                }
                match submit_ev::tui_run_submit_ev(
                    trimmed,
                    submit_ev::TuiSubmitEv {
                        clarify_shared: &clarify_shared,
                        cfg_holder,
                        config_path,
                        client,
                        tools,
                        messages: &mut messages,
                        work_dir: &mut work_dir,
                        cli_no_stream,
                        agent_role_owned: &mut agent_role_owned,
                        slash_handles: &slash_handles,
                        model: &model,
                        handoff_tx: &handoff_tx,
                        llm_scratch: &llm_scratch,
                        style: &style,
                        api_key_holder: &api_key_holder,
                        cli_rt: &cli_rt,
                        initial_pending: initial_pending.clone(),
                        process_handles: Arc::clone(&process_handles),
                        clarification_questionnaire_hook: Arc::clone(
                            &clarification_questionnaire_hook,
                        ),
                    },
                )
                .await?
                {
                    submit_ev::TuiSubmitHandled::SlashOnly => continue,
                    submit_ev::TuiSubmitHandled::RanRound => {}
                }
            }
            UiEvent::WorkspaceSwitch(raw) => {
                workspace_switch::tui_event_workspace_switch(
                    raw,
                    workspace_switch::TuiWorkspaceUiSwitch {
                        cfg_holder,
                        work_dir: &mut work_dir,
                        model: &model,
                        agent_role_owned: &agent_role_owned,
                        message_count: messages.len(),
                        tool_count: tools.len(),
                        cli_no_stream,
                    },
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

enum TuiPollKeyFlow {
    BreakLoop,
    ContinueOuter,
}

fn open_workspace_modal(model: &Arc<Mutex<TuiModel>>) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let initial = g.workspace_path_buf.clone();
    g.workspace_modal = Some(workspace_modal::TuiWorkspaceModalState::open(initial));
}

fn tui_dispatch_mouse(
    model: &Arc<Mutex<TuiModel>>,
    mouse: event::MouseEvent,
    llm_scratch: &TuiLlmStreamScratchArc,
) {
    let modal_open = {
        let g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.approval_modal.is_some() || g.clarification_modal.is_some() || g.workspace_modal.is_some()
    };
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
            g.chat_scrollbar_dragging = false;
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
        MouseEventKind::Up(_) => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            g.chat_scrollbar_dragging = false;
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let dragging = {
                let g = model.lock().unwrap_or_else(|e| e.into_inner());
                g.chat_scrollbar_dragging
            };
            if !dragging {
                return;
            }
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            let scratch = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(hit) =
                render::chat_scrollbar_hit(layout.chat, g.transcript.as_str(), &scratch)
            {
                g.focus = TuiFocus::Chat;
                g.chat_scroll_y = render::scrollbar_row_to_scroll_y(mouse.row, &hit);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let consumed_by_scrollbar = {
                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                let scratch = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(hit) =
                    render::chat_scrollbar_hit(layout.chat, g.transcript.as_str(), &scratch)
                {
                    if rect_contains(hit.rect, mouse.column, mouse.row) {
                        g.chat_scrollbar_dragging = true;
                        g.focus = TuiFocus::Chat;
                        g.chat_scroll_y = render::scrollbar_row_to_scroll_y(mouse.row, &hit);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if consumed_by_scrollbar {
                return;
            }
            if let Some(f) = focus_at_point(&layout, mouse.column, mouse.row) {
                let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
                g.focus = f;
                g.chat_scrollbar_dragging = false;
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
    match workspace_modal::handle_workspace_modal_keys(model, ev_tx, key) {
        workspace_modal::WorkspaceModalKeyOutcome::NotApplicable => {}
        workspace_modal::WorkspaceModalKeyOutcome::Consumed => {
            return TuiPollKeyFlow::ContinueOuter;
        }
    }
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
            let workspace_enter = {
                let g = model.lock().unwrap_or_else(|e| e.into_inner());
                g.focus == TuiFocus::SideRight
            };
            if workspace_enter {
                open_workspace_modal(model);
                return TuiPollKeyFlow::ContinueOuter;
            }
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
