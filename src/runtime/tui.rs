use crate::config::AgentConfig;
use crate::api::stream_chat;
use crate::types::{CommandApprovalDecision, Message};
use crate::types::ChatRequest;
use regex::Regex;
use std::sync::LazyLock;
use ratatui::termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseEvent, MouseButtons};
use ratatui::termwiz::terminal::Terminal as TermwizTerminal;
use ratatui::{
    backend::TermwizBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Terminal,
};
use ratatui::widgets::Padding;
use std::path::Path;
use std::time::{Duration, Instant};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use std::collections::HashSet;
use tui_markdown::{from_str_with_options as markdown_to_text, Options, StyleSheet};
use unicodeit::replace as latex_to_unicode;
use unicode_width::UnicodeWidthStr;
use ratatui::layout::Margin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    ChatView,
    ChatInput,
    Workspace,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    FileView,
    Prompt,
    CommandApprove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RightTab {
    Workspace = 0,
    Tasks = 1,
    Schedule = 2,
}

impl RightTab {
    fn titles() -> [&'static str; 3] {
        ["工作区", "任务", "日程"]
    }
}

/// 与 `sse_protocol` 对齐的 SSE 控制行；无法识别则视为模型流式正文。
#[derive(Debug)]
enum AgentLineKind {
    ToolRunning(bool),
    WorkspaceRefresh,
    CommandApproval {
        command: String,
        args: String,
    },
    StreamError,
    /// 已识别为协议行但无需刷新 UI（如 workspace_changed:false）
    Ignore,
    Plain,
}

fn classify_agent_sse_line(s: &str) -> AgentLineKind {
    if let Ok(msg) = serde_json::from_str::<crate::sse_protocol::SseMessage>(s) {
        match msg.payload {
            crate::sse_protocol::SsePayload::ToolRunning { tool_running } => {
                return AgentLineKind::ToolRunning(tool_running);
            }
            crate::sse_protocol::SsePayload::WorkspaceChanged {
                workspace_changed: true,
            } => return AgentLineKind::WorkspaceRefresh,
            crate::sse_protocol::SsePayload::WorkspaceChanged {
                workspace_changed: false,
            } => return AgentLineKind::Ignore,
            crate::sse_protocol::SsePayload::CommandApproval {
                command_approval_request,
            } => {
                return AgentLineKind::CommandApproval {
                    command: command_approval_request.command,
                    args: command_approval_request.args,
                };
            }
            crate::sse_protocol::SsePayload::Error(_) => return AgentLineKind::StreamError,
            crate::sse_protocol::SsePayload::ToolCall { .. }
            | crate::sse_protocol::SsePayload::ToolResult { .. }
            | crate::sse_protocol::SsePayload::PlanRequired { .. } => {
                return AgentLineKind::Ignore;
            }
        }
    }
    if s == r#"{"tool_running":true}"# {
        return AgentLineKind::ToolRunning(true);
    }
    if s == r#"{"tool_running":false}"# {
        return AgentLineKind::ToolRunning(false);
    }
    if s == r#"{"workspace_changed":true}"# {
        return AgentLineKind::WorkspaceRefresh;
    }
    if s.starts_with("{\"error\"") {
        return AgentLineKind::StreamError;
    }
    if s.starts_with("{\"command_approval_request\"")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
            && let Some(obj) = v.get("command_approval_request") {
                return AgentLineKind::CommandApproval {
                    command: obj
                        .get("command")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    args: obj
                        .get("args")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
            }
    AgentLineKind::Plain
}

fn draw_rect_corners(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    tl: &'static str,
    tr: &'static str,
    bl: &'static str,
    br: &'static str,
    style: Style,
) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let buf = f.buffer_mut();
    let x0 = rect.x;
    let x1 = rect.x + rect.width.saturating_sub(1);
    let y0 = rect.y;
    let y1 = rect.y + rect.height.saturating_sub(1);

    if let Some(cell) = buf.cell_mut((x0, y0)) {
        cell.set_symbol(tl);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x1, y0)) {
        cell.set_symbol(tr);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x0, y1)) {
        cell.set_symbol(bl);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x1, y1)) {
        cell.set_symbol(br);
        cell.set_style(style);
    }
}

fn right_tab_color(tab: RightTab) -> Color {
    match tab {
        RightTab::Workspace => Color::Green,
        RightTab::Tasks => Color::Yellow,
        RightTab::Schedule => Color::Cyan,
    }
}

#[derive(Debug, Clone)]
struct DarkStyleSheet;

#[derive(Debug, Clone)]
struct LightStyleSheet;

#[derive(Debug, Clone)]
struct HighContrastDarkStyleSheet;

#[derive(Debug, Clone)]
struct HighContrastLightStyleSheet;

impl StyleSheet for DarkStyleSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new().bold()
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::White)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Yellow)
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::LightYellow)
    }
}

impl StyleSheet for LightStyleSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new().bold()
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black)
    }

    fn link(&self) -> Style {
        Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Magenta)
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }
}

impl StyleSheet for HighContrastDarkStyleSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::White).bg(Color::Black)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::LightYellow)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::LightCyan)
    }
}

impl StyleSheet for HighContrastLightStyleSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new().fg(Color::Black).add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black).bg(Color::White)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::Black)
    }
}

fn code_themes() -> [&'static str; 5] {
    [
        "base16-ocean.dark",
        "base16-ocean.light",
        "Solarized (dark)",
        "Solarized (light)",
        "InspiredGitHub",
    ]
}

fn focus_name(f: Focus) -> &'static str {
    match f {
        Focus::ChatView => "聊天区",
        Focus::ChatInput => "输入区",
        Focus::Workspace => "工作区",
        Focus::Right => "右侧面板",
    }
}

struct TuiState {
    // chat
    messages: Vec<Message>,
    input: String,
    prompt: String,
    prompt_title: String,
    pending_command: String,
    pending_command_args: String,
    approve_choice: u8, // 0=拒绝 1=本次允许 2=永久允许
    persistent_command_allowlist: HashSet<String>,
    allowlist_file: std::path::PathBuf,
    // runtime
    status_line: String,
    tool_running: bool,
    focus: Focus,
    mode: Mode,
    // right panel
    tab: RightTab,
    // workspace view
    workspace_dir: std::path::PathBuf,
    workspace_entries: Vec<(String, bool)>, // (name, is_dir)
    workspace_sel: usize,
    // file view
    file_view_title: String,
    file_view_content: String,
    // tasks view
    task_items: Vec<(String, String, bool)>, // (id,title,done)
    task_sel: usize,
    // schedule view (reminders)
    reminder_items: Vec<(String, String, bool, Option<String>)>, // (id,title,done,due_at)
    reminder_sel: usize,
    // schedule view (events)
    event_items: Vec<(String, String, String)>, // (id,title,start_at)
    event_sel: usize,
    schedule_sub: u8, // 0=reminders, 1=events
    // markdown rendering
    md_style: u8, // 0=dark, 1=light
    high_contrast: bool,
    code_theme_idx: usize,
    // help overlay
    show_help: bool,
    // input area height (in terminal rows)
    input_rows: u16,
    input_dragging: bool,
    input_drag_row: u16,
    // chat scroll offset (0 = bottom, >0 = scrolled up)
    chat_scroll: i32,
    // When mouse clicks into ChatInput, we can override cursor position once.
    // This helps avoid terminals that visually "flash" when cursor jumps far.
    cursor_override: Option<(u16, u16)>,
    // Defer mouse-induced focus/tab changes until mouse release.
    // This avoids "mouse-down" visual flicker on some terminals.
    pending_focus: Option<Focus>,
    pending_tab: Option<RightTab>,
    // For mouse-down flicker: terminals may temporarily move their own cursor to
    // the clicked cell. We mirror that position for the next draw to avoid
    // hide/show or position-jump visuals.
    cursor_mouse_pos: Option<(u16, u16)>,
    /// termwiz/终端在开启鼠标报告时，偶发把 SGR 序列尾部（如 `<64;12;34M`）当普通字符送入输入。
    mouse_leak_scratch: String,
}

/// xterm SGR 鼠标报告：`\x1b[<btn;col;rowM`；若 CSI 被吞掉，可见部分形如 `<35;50;30M`。
static SGR_MOUSE_LEAK_TAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<\d+;\d+;\d+[Mm]$").expect("SGR mouse tail regex"));

static SGR_MOUSE_LEAK_EMBEDDED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[<\d+;\d+;\d+[Mm]|<\d+;\d+;\d+[Mm]").expect("SGR mouse embedded regex")
});

fn strip_sgr_mouse_leaks(s: &str) -> String {
    SGR_MOUSE_LEAK_EMBEDDED.replace_all(s, "").into_owned()
}

/// 丢弃误送入的 SGR 鼠标片段；否则将 `scratch` 与当前字符按用户输入写入 `push`。
fn feed_char_filter_sgr_mouse_leak<F: FnMut(char)>(scratch: &mut String, ch: char, mut push: F) {
    const MAX: usize = 32;
    if scratch.len() >= MAX {
        let old = std::mem::take(scratch);
        for c in old.chars() {
            push(c);
        }
    }
    if scratch.is_empty() {
        if ch == '<' {
            scratch.push(ch);
        } else {
            push(ch);
        }
        return;
    }
    let valid_next = ch.is_ascii_digit() || ch == ';' || ch == 'M' || ch == 'm';
    if !valid_next {
        let old = std::mem::take(scratch);
        for c in old.chars() {
            push(c);
        }
        push(ch);
        return;
    }
    scratch.push(ch);
    if ch == 'M' || ch == 'm' {
        if SGR_MOUSE_LEAK_TAIL.is_match(scratch.as_str()) {
            scratch.clear();
        } else {
            let old = std::mem::take(scratch);
            for c in old.chars() {
                push(c);
            }
        }
    }
}

fn command_approval_message(command: &str, args: &str) -> String {
    if args.trim().is_empty() {
        format!("命令审批：{}", command)
    } else {
        format!("命令审批：{} {}", command, args)
    }
}

fn load_persistent_allowlist(path: &Path) -> HashSet<String> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return HashSet::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&s) {
        Ok(v) => v,
        Err(_) => return HashSet::new(),
    };
    v.get("commands")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

fn save_persistent_allowlist(path: &Path, allowlist: &HashSet<String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut items = allowlist.iter().cloned().collect::<Vec<_>>();
    items.sort();
    let body = serde_json::json!({ "commands": items });
    if let Ok(s) = serde_json::to_string_pretty(&body) {
        let _ = std::fs::write(path, s.as_bytes());
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

    // initial messages
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
        status_line: format!(
            "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：输入）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
            cfg.model
        ),
        tool_running: false,
        focus: Focus::ChatInput,
        mode: Mode::Normal,
        tab: RightTab::Workspace,
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

    // terminal init
    // TermwizBackend will enable raw mode and alternate screen automatically.
    let backend = TermwizBackend::new()?;
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // agent output channel（流式正文与控制面 JSON 行）
    let (tx, mut rx) = mpsc::channel::<String>(2048);
    // 每轮 agent 结束后回传权威对话历史，与后台任务内的 `messages` 对齐（含 tool_calls / tool）
    let (sync_tx, mut sync_rx) = mpsc::channel::<Vec<Message>>(1);
    let mut approval_tx: Option<mpsc::Sender<CommandApprovalDecision>> = None;
    let mut agent_running: Option<tokio::task::JoinHandle<()>> = None;
    let mut assistant_buf = String::new();

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        // 先应用回合结束后的完整 messages，避免后续控制行写入过期的 assistant_buf
        while let Ok(msgs) = sync_rx.try_recv() {
            state.messages = msgs;
            assistant_buf.clear();
        }
        // pump agent stream into UI state
        while let Ok(s) = rx.try_recv() {
            match classify_agent_sse_line(&s) {
                AgentLineKind::ToolRunning(true) => {
                    state.tool_running = true;
                    state.status_line = "工具运行中…".to_string();
                }
                AgentLineKind::ToolRunning(false) => {
                    state.tool_running = false;
                    if state.status_line == "工具运行中…" {
                        state.status_line = format!(
                            "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                            cfg.model,
                            focus_name(state.focus)
                        );
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

        // draw
        terminal.draw(|f| draw_ui(f, &state))?;
        // Clear one-shot cursor overrides after the frame is rendered.
        state.cursor_mouse_pos = None;
        state.cursor_override = None;

        // finish agent task if done
        if let Some(handle) = agent_running.as_ref()
            && handle.is_finished() {
                agent_running = None;
                approval_tx = None;
                state.status_line = format!(
                    "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                    cfg.model,
                    focus_name(state.focus)
                );
            }

        // input events (termwiz backend)
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
                    handle_mouse(m, &mut state, screen_size.width, screen_size.height);
                }
                InputEvent::Resized { .. } => {
                    // ignore, layout will be recomputed on next draw
                }
                InputEvent::Paste(_) | InputEvent::Wake | InputEvent::PixelMouse(_) => {
                    // currently no-op
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_turn_tui(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &AgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
    no_stream: bool,
    persistent_allowlist: HashSet<String>,
    approval_rx: mpsc::Receiver<CommandApprovalDecision>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let out_tx = out.cloned();
    let out_tx_cloned = out_tx.clone();
    let approval_rx_shared: Arc<Mutex<mpsc::Receiver<CommandApprovalDecision>>> =
        Arc::new(Mutex::new(approval_rx));
    let approval_request_guard = Arc::new(Mutex::new(()));
    let persistent_allowlist_shared = Arc::new(Mutex::new(persistent_allowlist));
    let mut per_coord =
        crate::per_coord::PerCoordinator::new(cfg.reflection_default_max_rounds);
    'outer: loop {
        let req = ChatRequest {
            model: cfg.model.clone(),
            messages: messages.clone(),
            tools: Some(tools.to_vec()),
            tool_choice: Some("auto".to_string()),
            max_tokens: cfg.max_tokens,
            temperature: cfg.temperature,
            stream: None,
        };
        let (msg, finish_reason) =
            stream_chat(client, api_key, &cfg.api_base, &req, out, false, no_stream).await?;
        messages.push(msg.clone());
        if finish_reason != "tool_calls" {
            match per_coord.after_final_assistant(&msg) {
                crate::per_coord::AfterFinalAssistant::StopTurn => break,
                crate::per_coord::AfterFinalAssistant::RequestPlanRewrite(m) => {
                    messages.push(m);
                    continue 'outer;
                }
            }
        }
        let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
        let tui_tool_ctx = crate::tool_registry::TuiToolRuntime {
            out_tx: out_tx_cloned.clone(),
            approval_rx_shared: approval_rx_shared.clone(),
            approval_request_guard: approval_request_guard.clone(),
            persistent_allowlist_shared: persistent_allowlist_shared.clone(),
        };
        if let Some(tx) = out_tx_cloned.as_ref() {
            let _ = tx
                .send(crate::sse_protocol::encode_message(
                    crate::sse_protocol::SsePayload::ToolRunning {
                        tool_running: true,
                    },
                ))
                .await;
        }
        for tc in tool_calls {
            let name = tc.function.name.clone();
            let args = tc.function.arguments.clone();
            let id = tc.id.clone();
            if let Some(tx) = out_tx_cloned.as_ref()
                && let Some(summary) = crate::tools::summarize_tool_call(&name, &args) {
                    let _ = tx
                        .send(crate::sse_protocol::encode_message(
                            crate::sse_protocol::SsePayload::ToolCall {
                                tool_call: crate::sse_protocol::ToolCallSummary {
                                    name: name.clone(),
                                    summary,
                                },
                            },
                        ))
                        .await;
                }
            let (result, reflection_inject) = crate::tool_registry::dispatch_tool(
                crate::tool_registry::ToolRuntime::Tui {
                    ctx: &tui_tool_ctx,
                },
                &mut per_coord,
                cfg,
                effective_working_dir,
                workspace_is_set,
                &name,
                &args,
                tc,
            )
            .await;
            if let Some(tx) = out {
                if tx.is_closed() {
                    break 'outer;
                }
                let _ = tx
                    .send(crate::sse_protocol::encode_message(
                        crate::sse_protocol::SsePayload::ToolResult {
                            tool_result: crate::sse_protocol::ToolResultBody {
                                name: tc.function.name.clone(),
                                output: result.clone(),
                            },
                        },
                    ))
                    .await;
            }
            crate::per_coord::PerCoordinator::append_tool_result_and_reflection(
                messages,
                id,
                result,
                reflection_inject,
            );
        }
        if let Some(tx) = out {
            let _ = tx
                .send(crate::sse_protocol::encode_message(
                    crate::sse_protocol::SsePayload::ToolRunning {
                        tool_running: false,
                    },
                ))
                .await;
        }
    }
    Ok(())
}

/// TUI 按键处理所需的 Agent / 通道 / 配置上下文，避免 `handle_key` 参数过长。
struct HandleKeyContext<'a> {
    agent_running: &'a mut Option<tokio::task::JoinHandle<()>>,
    assistant_buf: &'a mut String,
    approval_tx: &'a mut Option<mpsc::Sender<CommandApprovalDecision>>,
    tx: &'a mpsc::Sender<String>,
    sync_tx: mpsc::Sender<Vec<Message>>,
    cfg: &'a AgentConfig,
    client: &'a reqwest::Client,
    api_key: &'a str,
    tools: &'a [crate::types::Tool],
    no_stream: bool,
}

async fn handle_key(
    key: KeyEvent,
    state: &mut TuiState,
    ctx: HandleKeyContext<'_>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let HandleKeyContext {
        agent_running,
        assistant_buf,
        approval_tx,
        tx,
        sync_tx,
        cfg,
        client,
        api_key,
        tools,
        no_stream,
    } = ctx;
    // exit
    if key.key == KeyCode::Char('c') && key.modifiers.contains(Modifiers::CTRL) {
        return Ok(true);
    }

    // Any keyboard activity should cancel mouse-down cursor mirroring.
    state.cursor_mouse_pos = None;

    // modal / prompt
    if state.mode == Mode::FileView {
        match key.key {
            KeyCode::Escape | KeyCode::Char('q') => {
                state.mode = Mode::Normal;
                state.file_view_title.clear();
                state.file_view_content.clear();
            }
            _ => {}
        }
        return Ok(false);
    }
    if state.mode == Mode::Prompt {
        match key.key {
            KeyCode::Escape => {
                state.mode = Mode::Normal;
                state.mouse_leak_scratch.clear();
                state.prompt.clear();
                state.prompt_title.clear();
            }
            KeyCode::Enter => {
                // confirm prompt based on title
                if state.prompt_title.starts_with("新增提醒") {
                    // format: "title @ due_at" (due_at optional)
                    let raw = state.prompt.trim();
                    if !raw.is_empty() {
                        let (title, due_at) = split_title_due(raw);
                        let args = if let Some(d) = due_at {
                            serde_json::json!({ "title": title, "due_at": d }).to_string()
                        } else {
                            serde_json::json!({ "title": title }).to_string()
                        };
                        // call tool directly for consistency
                        let _ = crate::tools::run_tool(
                            "add_reminder",
                            &args,
                            cfg.command_max_output_len,
                            cfg.weather_timeout_secs,
                            &cfg.allowed_commands,
                            &state.workspace_dir,
                        );
                        refresh_schedule(state);
                    }
                }
                state.mode = Mode::Normal;
                state.mouse_leak_scratch.clear();
                state.prompt.clear();
                state.prompt_title.clear();
            }
            KeyCode::Backspace => {
                if !state.mouse_leak_scratch.is_empty() {
                    state.mouse_leak_scratch.pop();
                } else {
                    state.prompt.pop();
                }
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(Modifiers::CTRL) && !key.modifiers.contains(Modifiers::ALT) {
                    feed_char_filter_sgr_mouse_leak(&mut state.mouse_leak_scratch, ch, |c| {
                        state.prompt.push(c);
                    });
                }
            }
            _ => {}
        }
        return Ok(false);
    }
    if state.mode == Mode::CommandApprove {
        async fn submit_approval(
            state: &mut TuiState,
            approval_tx: &mut Option<mpsc::Sender<CommandApprovalDecision>>,
            cfg: &AgentConfig,
            decision: CommandApprovalDecision,
        ) {
            if let CommandApprovalDecision::AllowAlways = decision {
                let cmd = state.pending_command.trim().to_lowercase();
                if !cmd.is_empty() {
                    state.persistent_command_allowlist.insert(cmd);
                    save_persistent_allowlist(
                        &state.allowlist_file,
                        &state.persistent_command_allowlist,
                    );
                }
            }
            if let Some(ch) = approval_tx.as_ref() {
                let _ = ch.send(decision).await;
            }
            state.mode = Mode::Normal;
            state.status_line = format!(
                "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                cfg.model,
                focus_name(state.focus)
            );
            state.pending_command.clear();
            state.pending_command_args.clear();
        }

        match key.key {
            KeyCode::Escape => {
                state.approve_choice = 0;
            }
            KeyCode::LeftArrow => {
                state.approve_choice = state.approve_choice.saturating_sub(1);
            }
            KeyCode::RightArrow => {
                state.approve_choice = (state.approve_choice + 1).min(2);
            }
            KeyCode::Char('1') => state.approve_choice = 0,
            KeyCode::Char('2') => state.approve_choice = 1,
            KeyCode::Char('3') => state.approve_choice = 2,
            // 快捷键：d=拒绝，o=本次允许，p=永久允许（按下即确认）
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'d') => {
                submit_approval(
                    state,
                    approval_tx,
                    cfg,
                    CommandApprovalDecision::Deny,
                )
                .await;
            }
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'o') => {
                submit_approval(
                    state,
                    approval_tx,
                    cfg,
                    CommandApprovalDecision::AllowOnce,
                )
                .await;
            }
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'p') => {
                submit_approval(
                    state,
                    approval_tx,
                    cfg,
                    CommandApprovalDecision::AllowAlways,
                )
                .await;
            }
            KeyCode::Enter => {
                let decision = match state.approve_choice {
                    1 => CommandApprovalDecision::AllowOnce,
                    2 => CommandApprovalDecision::AllowAlways,
                    _ => CommandApprovalDecision::Deny,
                };
                submit_approval(state, approval_tx, cfg, decision).await;
            }
            _ => {}
        }
        return Ok(false);
    }

    // 全局帮助弹窗优先处理（除 Ctrl+C 外）
    if state.show_help {
        match key.key {
            KeyCode::Function(1) | KeyCode::Escape => {
                state.show_help = false;
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.key {
        KeyCode::Function(1) => {
            state.show_help = !state.show_help;
        }
        KeyCode::Function(2) => {
            // Keyboard focus switching doesn't carry a click position.
            state.cursor_override = None;
            state.focus = match state.focus {
                Focus::ChatView => Focus::ChatInput,
                Focus::ChatInput => Focus::Workspace,
                Focus::Workspace => Focus::Right,
                Focus::Right => Focus::ChatView,
            };
            state.status_line = format!(
                "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                cfg.model,
                focus_name(state.focus)
            );
        }
        KeyCode::Function(3) => {
            state.code_theme_idx = (state.code_theme_idx + 1) % code_themes().len();
            state.status_line = format!("代码主题：{}（F3 切换）", code_themes()[state.code_theme_idx]);
        }
        KeyCode::Function(4) => {
            state.md_style = if state.md_style == 0 { 1 } else { 0 };
            state.status_line = format!(
                "Markdown样式：{}（F4 切换）",
                if state.md_style == 0 { "dark" } else { "light" }
            );
        }
        KeyCode::Function(5) => {
            state.high_contrast = !state.high_contrast;
            state.status_line = format!(
                "高对比度：{}（F5 切换）  |  模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                if state.high_contrast { "开" } else { "关" },
                cfg.model,
                focus_name(state.focus)
            );
        }
        KeyCode::PageUp => {
            // 向上翻一屏（不再强制要求聊天聚焦，避免误锁死）
            let step = state.input_rows.max(3) as i32; // 约一屏高度
            state.chat_scroll += step;
        }
        KeyCode::PageDown => {
            let step = state.input_rows.max(3) as i32;
            state.chat_scroll -= step;
            if state.chat_scroll < 0 {
                state.chat_scroll = 0;
            }
        }
        KeyCode::Tab => {
            state.tab = match state.tab {
                RightTab::Workspace => RightTab::Tasks,
                RightTab::Tasks => RightTab::Schedule,
                RightTab::Schedule => RightTab::Workspace,
            };
            // 只有在工作区标签时才允许“工作区焦点”
            if state.focus == Focus::Workspace && state.tab != RightTab::Workspace {
                state.focus = Focus::Right;
            }
            // refresh view data on tab switch
            match state.tab {
                RightTab::Workspace => refresh_workspace(state),
                RightTab::Tasks => refresh_tasks(state),
                RightTab::Schedule => refresh_schedule(state),
            }
        }
        KeyCode::UpArrow => {
            if state.focus == Focus::Right || state.focus == Focus::Workspace {
                match state.tab {
                    RightTab::Workspace => {
                        if !state.workspace_entries.is_empty() {
                            state.workspace_sel = state.workspace_sel.saturating_sub(1);
                        }
                    }
                    RightTab::Tasks => {
                        if !state.task_items.is_empty() {
                            state.task_sel = state.task_sel.saturating_sub(1);
                        }
                    }
                    RightTab::Schedule => {
                        if state.schedule_sub == 0 {
                            if !state.reminder_items.is_empty() {
                                state.reminder_sel = state.reminder_sel.saturating_sub(1);
                            }
                        } else if !state.event_items.is_empty() {
                            state.event_sel = state.event_sel.saturating_sub(1);
                        }
                    }
                }
            }
        }
        KeyCode::DownArrow => {
            if state.focus == Focus::Right || state.focus == Focus::Workspace {
                match state.tab {
                    RightTab::Workspace => {
                        if !state.workspace_entries.is_empty() {
                            state.workspace_sel =
                                (state.workspace_sel + 1).min(state.workspace_entries.len() - 1);
                        }
                    }
                    RightTab::Tasks => {
                        if !state.task_items.is_empty() {
                            state.task_sel = (state.task_sel + 1).min(state.task_items.len() - 1);
                        }
                    }
                    RightTab::Schedule => {
                        if state.schedule_sub == 0 {
                            if !state.reminder_items.is_empty() {
                                state.reminder_sel =
                                    (state.reminder_sel + 1).min(state.reminder_items.len() - 1);
                            }
                        } else if !state.event_items.is_empty() {
                            state.event_sel = (state.event_sel + 1).min(state.event_items.len() - 1);
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            // refresh right panel data
            refresh_workspace(state);
            refresh_tasks(state);
            refresh_schedule(state);
        }
        KeyCode::Enter => {
            if state.focus == Focus::Right || state.focus == Focus::Workspace {
                match state.tab {
                    RightTab::Workspace => {
                        workspace_open_or_enter(state);
                    }
                    RightTab::Tasks => {
                        // no-op (use Space to toggle)
                    }
                    RightTab::Schedule => {
                        // no-op
                    }
                }
                return Ok(false);
            }
            // send chat
            if agent_running.is_none() && state.focus == Focus::ChatInput {
                let q = state.input.trim().to_string();
                if !q.is_empty() {
                    state.mouse_leak_scratch.clear();
                    state.cursor_override = None;
                    state.input.clear();
                    // 若当前在底部，则保持自动滚动；否则保留用户的历史查看位置
                    if state.chat_scroll <= 0 {
                        state.chat_scroll = 0;
                    }
                    state.messages.push(Message {
                        role: "user".to_string(),
                        content: Some(q),
                        tool_calls: None,
                        name: None,
                        tool_call_id: None,
                    });
                    // reset assistant buffer and create placeholder assistant msg
                    assistant_buf.clear();
                    state.messages.push(Message {
                        role: "assistant".to_string(),
                        content: Some(String::new()),
                        tool_calls: None,
                        name: None,
                        tool_call_id: None,
                    });
                    state.status_line = "模型生成中…".to_string();
                    let mut messages = state.messages.clone();
                    let tx2 = tx.clone();
                    let work_dir = state.workspace_dir.clone();
                    let workspace_is_set = true; // TUI 以 CLI/work_dir 为准，视为已设置
                    let cfg = cfg.clone();
                    let client = client.clone();
                    let api_key = api_key.to_string();
                    let tools = tools.to_vec();
                    let persistent_allowlist = state.persistent_command_allowlist.clone();
                    let (approve_tx_ch, approve_rx_ch) =
                        mpsc::channel::<CommandApprovalDecision>(8);
                    *approval_tx = Some(approve_tx_ch);
                    let sync_tx2 = sync_tx.clone();
                    *agent_running = Some(tokio::spawn(async move {
                        let out = Some(&tx2);
                        let res = run_agent_turn_tui(
                            &client,
                            &api_key,
                            &cfg,
                            &tools,
                            &mut messages,
                            out,
                            &work_dir,
                            workspace_is_set,
                            no_stream,
                            persistent_allowlist,
                            approve_rx_ch,
                        )
                        .await;
                        let _ = sync_tx2.send(messages).await;
                        if let Err(e) = res {
                            let _ = tx2
                                .send(crate::sse_protocol::encode_message(
                                    crate::sse_protocol::SsePayload::Error(
                                        crate::sse_protocol::SseErrorBody {
                                            error: e.to_string(),
                                            code: Some("AGENT_TURN".to_string()),
                                        },
                                    ),
                                ))
                                .await;
                        }
                        let _ = tx2
                            .send(crate::sse_protocol::encode_message(
                                crate::sse_protocol::SsePayload::ToolRunning {
                                    tool_running: false,
                                },
                            ))
                            .await;
                    }));
                }
            }
        }
        KeyCode::Backspace => {
            if (state.focus == Focus::Right || state.focus == Focus::Workspace)
                && state.tab == RightTab::Workspace
            {
                workspace_go_up(state);
            } else if state.focus == Focus::ChatInput {
                state.cursor_override = None;
                if !state.mouse_leak_scratch.is_empty() {
                    state.mouse_leak_scratch.pop();
                } else {
                    state.input.pop();
                }
            }
        }
        KeyCode::Char(' ') => {
            if state.focus == Focus::Right {
                match state.tab {
                    RightTab::Tasks => {
                        toggle_task_done(state);
                    }
                    RightTab::Schedule => {
                        if state.schedule_sub == 0 {
                            toggle_reminder_done(state);
                        }
                    }
                    _ => {}
                }
            } else {
                state.cursor_override = None;
                feed_char_filter_sgr_mouse_leak(&mut state.mouse_leak_scratch, ' ', |c| {
                    state.input.push(c);
                });
            }
        }
        KeyCode::Char('a') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule && state.schedule_sub == 0 {
                state.mouse_leak_scratch.clear();
                state.mode = Mode::Prompt;
                state.prompt.clear();
                state.prompt_title = "新增提醒：输入「标题 @ 2026-03-20 09:00」（@ 后可省略）".to_string();
            }
        }
        KeyCode::Char('e') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule {
                state.schedule_sub = 1;
            }
        }
        KeyCode::Char('t') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule {
                state.schedule_sub = 0;
            }
        }
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(Modifiers::CTRL) && !key.modifiers.contains(Modifiers::ALT)
                && state.focus == Focus::ChatInput {
                    state.cursor_override = None;
                    feed_char_filter_sgr_mouse_leak(&mut state.mouse_leak_scratch, ch, |c| {
                        state.input.push(c);
                    });
                }
        }
        _ => {}
    }

    Ok(false)
}

fn upsert_assistant_message(messages: &mut Vec<Message>, content: &str) {
    if let Some(last) = messages.iter_mut().rev().find(|m| m.role == "assistant") {
        last.content = Some(content.to_string());
    } else {
        messages.push(Message {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });
    }
}

fn refresh_workspace(state: &mut TuiState) {
    let mut entries = Vec::new();
    let dir = &state.workspace_dir;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
            entries.push((name, is_dir));
        }
        entries.sort_by(|a, b| match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        });
    }
    state.workspace_entries = entries;
    state.workspace_sel = state.workspace_sel.min(state.workspace_entries.len().saturating_sub(1));
}

fn refresh_tasks(state: &mut TuiState) {
    let path = state.workspace_dir.join("tasks.json");
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            state.task_items = Vec::new();
            state.task_sel = 0;
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
    let items = v.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for it in items {
        let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let title = it.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let done = it.get("done").and_then(|x| x.as_bool()).unwrap_or(false);
        if !title.is_empty() {
            out.push((id, title, done));
        }
    }
    state.task_items = out;
    state.task_sel = state.task_sel.min(state.task_items.len().saturating_sub(1));
}

fn refresh_schedule(state: &mut TuiState) {
    // reminders
    let rpath = state.workspace_dir.join(".crabmate").join("reminders.json");
    let mut reminders = Vec::new();
    if let Ok(s) = std::fs::read_to_string(&rpath) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr.iter().take(200) {
                let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let title = it.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let done = it.get("done").and_then(|x| x.as_bool()).unwrap_or(false);
                let due_at = it.get("due_at").and_then(|x| x.as_str()).map(|s| s.to_string());
                if !title.is_empty() {
                    reminders.push((id, title, done, due_at));
                }
            }
        }
    }
    state.reminder_items = reminders;
    state.reminder_sel = state
        .reminder_sel
        .min(state.reminder_items.len().saturating_sub(1));

    // events
    let epath = state.workspace_dir.join(".crabmate").join("events.json");
    let mut events = Vec::new();
    if let Ok(s) = std::fs::read_to_string(&epath) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or(serde_json::json!({}));
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr.iter().take(200) {
                let id = it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let title = it.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let start = it.get("start_at").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if !title.is_empty() {
                    events.push((id, title, start));
                }
            }
        }
    }
    state.event_items = events;
    state.event_sel = state.event_sel.min(state.event_items.len().saturating_sub(1));
}

fn split_title_due(s: &str) -> (String, Option<String>) {
    // "title @ due"
    if let Some((a, b)) = s.split_once('@') {
        let title = a.trim().to_string();
        let due = b.trim().to_string();
        if due.is_empty() {
            (title, None)
        } else {
            (title, Some(due))
        }
    } else {
        (s.trim().to_string(), None)
    }
}

fn workspace_go_up(state: &mut TuiState) {
    if let Some(p) = state.workspace_dir.parent() {
        state.workspace_dir = p.to_path_buf();
        refresh_workspace(state);
        refresh_tasks(state);
        refresh_schedule(state);
    }
}

fn workspace_open_or_enter(state: &mut TuiState) {
    let Some((name, is_dir)) = state.workspace_entries.get(state.workspace_sel).cloned() else {
        return;
    };
    let path = state.workspace_dir.join(&name);
    if is_dir {
        state.workspace_dir = path;
        refresh_workspace(state);
        refresh_tasks(state);
        refresh_schedule(state);
        return;
    }
    // open file viewer
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| format!("读取失败：{}", e));
    let content = if content.len() > 200_000 {
        format!("{}\n\n...(内容过长已截断)", &content[..200_000])
    } else {
        content
    };
    state.file_view_title = path.display().to_string();
    state.file_view_content = content;
    state.mode = Mode::FileView;
}

fn toggle_task_done(state: &mut TuiState) {
    if state.task_items.is_empty() {
        return;
    }
    let idx = state.task_sel.min(state.task_items.len() - 1);
    state.task_items[idx].2 = !state.task_items[idx].2;
    // write back tasks.json (preserve optional source/updated_at if present)
    let path = state.workspace_dir.join("tasks.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let items: Vec<serde_json::Value> = state
        .task_items
        .iter()
        .map(|(id, title, done)| serde_json::json!({ "id": id, "title": title, "done": done }))
        .collect();
    root["items"] = serde_json::Value::Array(items);
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        let _ = std::fs::write(&path, s.as_bytes());
    }
}

fn toggle_reminder_done(state: &mut TuiState) {
    if state.reminder_items.is_empty() {
        return;
    }
    let idx = state.reminder_sel.min(state.reminder_items.len() - 1);
    state.reminder_items[idx].2 = !state.reminder_items[idx].2;
    let path = state.workspace_dir.join(".crabmate").join("reminders.json");
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let items: Vec<serde_json::Value> = state
        .reminder_items
        .iter()
        .map(|(id, title, done, due_at)| {
            let mut v = serde_json::json!({ "id": id, "title": title, "done": done });
            if let Some(d) = due_at {
                v["due_at"] = serde_json::Value::String(d.clone());
            }
            v
        })
        .collect();
    root["items"] = serde_json::Value::Array(items);
    // ensure dir exists
    let _ = std::fs::create_dir_all(state.workspace_dir.join(".crabmate"));
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        let _ = std::fs::write(&path, s.as_bytes());
    }
}

fn draw_ui(f: &mut ratatui::Frame<'_>, state: &TuiState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    draw_chat(f, chunks[0], state);
    draw_right(f, chunks[1], state);

    // 是否显示“手工粗分隔线”（━/┃）。
    // 你当前希望彻底隐藏这些线，因此默认关闭。
    const SHOW_SEPARATORS: bool = false;
    if SHOW_SEPARATORS {
        // 统一在最后覆盖绘制粗分隔线，确保窗口交界不留空隙
        // 说明：先画横线，再画竖线（竖线最后），以保证交点处是“┃”而不是被“━”覆盖。
        let sep_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD);

        // 左侧：聊天/输入/状态 的横分隔线
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(state.input_rows.max(2)),
                Constraint::Length(1),
            ])
            .split(chunks[0]);

        // chat<->input
        let left_sep1_y = left_chunks[1].y;
        for dy in 0..2u16 {
            let y = left_sep1_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[0].x, y, chunks[0].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[0].width as usize)).style(sep_style),
                sep_area,
            );
        }

        // input<->status
        let left_sep2_y = left_chunks[2].y;
        for dy in 0..2u16 {
            let y = left_sep2_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[0].x, y, chunks[0].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[0].width as usize)).style(sep_style),
                sep_area,
            );
        }

        // 右侧：标签与内容横分隔线
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(chunks[1]);

        let right_sep_y = right_chunks[1].y;
        for dy in 0..2u16 {
            let y = right_sep_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[1].x, y, chunks[1].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[1].width as usize)).style(sep_style),
                sep_area,
            );
        }

        // 左右主区域竖分隔线（最后绘制）
        let separator_x_start = chunks[1].x.saturating_sub(1);
        // 中央竖线覆盖 2 列
        for dx in 0..2u16 {
            let x = separator_x_start.saturating_add(dx);
            if x >= area.x.saturating_add(area.width) {
                continue;
            }
            let sep_area = Rect::new(x, area.y, 1, area.height);
            f.render_widget(Clear, sep_area);
            let vbar_lines: Vec<Line<'_>> =
                (0..sep_area.height).map(|_| Line::raw("┃")).collect();
            f.render_widget(Paragraph::new(vbar_lines).style(sep_style), sep_area);
        }
    }

    if state.mode == Mode::CommandApprove {
        let w = area.width.saturating_mul(7) / 10;
        let h: u16 = 9;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let popup = Rect::new(x, y, w.max(50), h);
        let options = ["拒绝", "本次允许", "永久允许"];
        let mut option_line: Vec<Span<'_>> = Vec::new();
        for (i, text) in options.iter().enumerate() {
            if i as u8 == state.approve_choice {
                option_line.push(Span::styled(
                    format!("[{}]", text),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ));
            } else {
                option_line.push(Span::raw(format!(" {} ", text)));
            }
            option_line.push(Span::raw("  "));
        }
        let args_text = if state.pending_command_args.trim().is_empty() {
            "(无参数)".to_string()
        } else {
            state.pending_command_args.clone()
        };
        let lines = vec![
            Line::from(Span::styled(
                "命令执行审批",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(format!("命令: {}", state.pending_command)),
            Line::raw(format!("参数: {}", args_text)),
            Line::raw(""),
            Line::from(option_line),
            Line::raw("←/→ 选择，Enter 确认（1/2/3 选项，Esc=拒绝）"),
            Line::raw("快捷键：D=拒绝，O=本次允许，P=永久允许（按下即确认）"),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .title(" 命令审批 ");
        let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        f.render_widget(Clear, popup);
        f.render_widget(para, popup);
    }

    if state.show_help {
        // 居中弹窗区域（约 70% 宽、80% 高）
        let w = area.width.saturating_mul(7) / 10;
        let h = area.height.saturating_mul(8) / 10;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let popup = Rect::new(x, y, w.max(40), h.max(15));

        let help_lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Crabmate TUI 小教程",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from("布局：左侧对话 + 输入区域，右侧 工作区 / 任务 / 日程 标签页。"),
            Line::from("焦点切换：F2 在 聊天 和 右侧 面板之间切换，Tab 在右侧标签页间切换。"),
            Line::from("发送：在输入框中按 Enter 发送消息；工具运行时状态栏会提示。"),
            Line::from("Markdown：F3 切换代码主题，F4 切换 Markdown 暗/亮样式。"),
            Line::from("高对比度：F5 在普通 / 高对比度模式之间切换（适合弱光/弱视）。"),
            Line::from("任务 / 日程：右侧标签页中查看和勾选任务、提醒和事件。"),
            Line::raw(""),
            Line::from("按 F1 或 Esc 关闭此帮助，随时再次按 F1 重新查看。"),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .title(" 帮助 / 教程 ");
        let para = Paragraph::new(help_lines)
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(Clear, popup);
        f.render_widget(para, popup);
    }
}

fn draw_chat(f: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(state.input_rows.max(2)),
            Constraint::Length(1),
        ])
        .split(area);

    let mut lines: Vec<Line<'_>> = Vec::new();
    let chat_inner_width = vchunks[0].width.saturating_sub(2) as usize;
    // 先把本次 draw_chat 里要渲染的消息 + latex_to_unicode 结果一次性算出来，
    // 再生成 Line<'_>（Line 内部引用的 &str 生命周期由 rendered_list 持有，且 draw_chat 期间不会再被修改）。
    let chat_msgs: Vec<&Message> = state.messages.iter().filter(|m| m.role != "system").collect();
    let rendered_list: Vec<String> = chat_msgs
        .iter()
        .map(|m| {
            // 让 workflow_execute/tool 等“结构化 JSON 输出”回落为可读摘要，提升 TUI 可用性。
            let raw = m.content.as_deref().unwrap_or("");
            let display_raw = if m.role == "tool" {
                serde_json::from_str::<serde_json::Value>(raw)
                    .ok()
                    .and_then(|v| v.get("human_summary").and_then(|x| x.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| raw.to_string())
            } else {
                raw.to_string()
            };
            latex_to_unicode(&display_raw)
        })
        .collect();

    for (idx, m) in chat_msgs.iter().enumerate() {
        let role = if m.role == "user" { "我" } else { "模型" };
        let rendered = rendered_list[idx].as_str();
        if m.role == "user" {
            let role_text = format!("{}:", role);
            let role_padded = if role_text.width() >= chat_inner_width {
                role_text
            } else {
                format!(
                    "{}{}",
                    " ".repeat(chat_inner_width.saturating_sub(role_text.width())),
                    role_text
                )
            };
            lines.push(Line::from(Span::styled(
                role_padded,
                Style::default().add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}:", role),
                Style::default().add_modifier(Modifier::BOLD),
            )));
        }
        if m.role == "assistant" {
            // 使用 tui-markdown 渲染：链接会包含 URL（便于复制/终端自动识别点击）。
            let theme = code_themes()[state.code_theme_idx];
            let text = match (state.md_style, state.high_contrast) {
                (0, false) => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (0, true) => {
                    let options = Options::new(HighContrastDarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (1, false) => {
                    let options = Options::new(LightStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (1, true) => {
                    let options = Options::new(HighContrastLightStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                _ => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
            };
            lines.extend(text.lines.into_iter());
        } else {
            // user message: keep raw and right-align
            for l in rendered.lines() {
                let padded = if l.width() >= chat_inner_width {
                    l.to_string()
                } else {
                    format!(
                        "{}{}",
                        " ".repeat(chat_inner_width.saturating_sub(l.width())),
                        l
                    )
                };
                lines.push(Line::raw(padded));
            }
        }
        lines.push(Line::raw("")); // spacer
    }
    // 根据可用高度和滚动偏移选择显示的行区间
    let chat_height = vchunks[0].height.saturating_sub(2); // 去掉边框的大致可用行数
    if chat_height > 0 && !lines.is_empty() {
        let total = lines.len() as i32;
        let height = chat_height as i32;
        let max_offset = (total - height).max(0);
        let offset = state.chat_scroll.clamp(0, max_offset);
        let start = offset as usize;
        let end = (offset + height).min(total) as usize;
        lines = lines[start..end].to_vec();
    }
    // 顶部聊天区：使用角标替代边框线
    let chat_focused = state.focus == Focus::ChatView;
    let chat_block = Block::default()
        .borders(Borders::NONE)
        // 给内容预留 1 格空边，避免内容覆盖角标
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(Color::Black));
    let chat = Paragraph::new(lines)
        .block(chat_block)
        .wrap(Wrap { trim: false });
    f.render_widget(chat, vchunks[0]);
    let chat_corner_style = Style::default()
        .fg(if chat_focused { Color::Cyan } else { Color::DarkGray })
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[0],
        "┏",
        "┓",
        "┗",
        "┛",
        chat_corner_style,
    );

    let input_text = if state.mode == Mode::Prompt {
        state.prompt.as_str()
    } else {
        state.input.as_str()
    };
    let input_focused = state.mode == Mode::Prompt || state.focus == Focus::ChatInput;
    // 输入框四个角用“角标”代替边框线
    let input_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(Color::DarkGray));
    let input = Paragraph::new(input_text)
        .block(input_block)
        // Use a stable fg/bg regardless of focus.
        // Some terminals (xfce4-terminal) show a brief local color flash when switching focus.
        .style(Style::default().fg(Color::Gray).bg(Color::DarkGray))
        .wrap(Wrap { trim: false });
    f.render_widget(input, vchunks[1]);

    // 绘制输入框四角（角标字符包含竖向边的语义）
    let input_corner_style = Style::default()
        .fg(if input_focused { Color::Yellow } else { Color::DarkGray })
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[1],
        "┏",
        "┓",
        "┗",
        "┛",
        input_corner_style,
    );

    // 光标刷新尽量只在“输入框聚焦”时做，避免模型滚动/切换焦点时
    // 光标位置被反复 set_cursor_position 导致 xfce4-terminal 局部背景闪烁。
    if state.mode != Mode::CommandApprove && !state.show_help {
        // mouse-down one-shot: mirror to the clicked cell (only armed when click is in input area)
        if let Some((mx, my)) = state.cursor_mouse_pos {
            let area = f.area();
            let max_x = area.x.saturating_add(area.width.saturating_sub(1));
            let max_y = area.y.saturating_add(area.height.saturating_sub(1));
            let x = mx.min(max_x);
            let y = my.min(max_y);
            f.set_cursor_position((x, y));
        } else if input_focused {
            // keep cursor inside input area when focused
            let inner = vchunks[1].inner(Margin { vertical: 1, horizontal: 1 });
            if inner.width > 0 && inner.height > 0 {
                if let Some((cx, cy)) = state.cursor_override {
                    // Put cursor near the click spot, clamped into the input content area.
                    let rel_x = cx.saturating_sub(inner.x);
                    let rel_y = cy.saturating_sub(inner.y);
                    let max_dx = inner.width.saturating_sub(1);
                    let max_dy = inner.height.saturating_sub(1);
                    let x = inner.x.saturating_add(rel_x.min(max_dx));
                    let y = inner.y.saturating_add(rel_y.min(max_dy));
                    f.set_cursor_position((x, y));
                } else {
                    // Default: cursor at end of current input text.
                    let lines: Vec<&str> = input_text.split('\n').collect();
                    let line_idx = lines.len().saturating_sub(1);
                    let last = lines.get(line_idx).copied().unwrap_or("");
                    let x_off = last.width() as u16;
                    let x = inner
                        .x
                        .saturating_add(x_off)
                        .min(inner.x + inner.width.saturating_sub(1));
                    let y = inner
                        .y
                        .saturating_add(line_idx as u16)
                        .min(inner.y + inner.height.saturating_sub(1));
                    f.set_cursor_position((x, y));
                }
            }
        }
    }

    // 状态区：使用角标替代边框线
    let status_color = match state.focus {
        Focus::ChatView => Color::Cyan,
        Focus::ChatInput => Color::Yellow,
        Focus::Workspace => Color::Green,
        Focus::Right => Color::Magenta,
    };
    let status_bg = Color::Blue;
    let status_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(status_bg));
    let status = Paragraph::new(state.status_line.as_str())
        .block(status_block)
        .style(Style::default().fg(status_color).bg(status_bg));
    f.render_widget(status, vchunks[2]);
    let status_corner_style = Style::default()
        .fg(status_color)
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[2],
        "┏",
        "┓",
        "┗",
        "┛",
        status_corner_style,
    );

}

fn handle_mouse(me: MouseEvent, state: &mut TuiState, cols: u16, rows: u16) {
    let x = me.x;
    let y = me.y;

    // 1) 滚轮事件（termwiz：Button4=wheel up，Button5=wheel down）
    if me.mouse_buttons.contains(MouseButtons::VERT_WHEEL) {
        if me.mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
            // wheel up
            state.chat_scroll -= 3;
            if state.chat_scroll < 0 {
                state.chat_scroll = 0;
            }
        } else {
            // wheel down
            state.chat_scroll += 3;
        }
        return;
    }

    // 2) 鼠标拖动/点击逻辑（只看左键按下状态）
    let left_pressed = me.mouse_buttons.contains(MouseButtons::LEFT);

    if state.input_dragging {
        // 拖动过程中：只要左键还按着，就按鼠标 y 增量调整输入高度
        if left_pressed {
            let prev = state.input_drag_row;
            let cur = y;
            if cur != prev {
                let delta = prev as i16 - cur as i16;
                let new_rows = (state.input_rows as i16 + delta).clamp(3, 12) as u16;
                state.input_rows = new_rows;
                state.input_drag_row = cur;
                state.status_line = format!(
                    "正在拖动输入区域高度（当前：{} 行）",
                    state.input_rows
                );
            }
        } else {
            // 松开左键：结束拖动
            state.input_dragging = false;
            state.status_line = format!(
                "输入区域高度已调整为 {} 行（在底部拖动可再次调整）",
                state.input_rows
            );
        }
        return;
    }

    // 非拖动：左键按下时做命中（点击聚焦/底部进入高度拖动模式）
    if left_pressed {
        let chat_width = cols.saturating_mul(65) / 100;
        let input_start_row = rows.saturating_sub(state.input_rows + 1);

        // One-shot cursor mirroring for the next draw after mouse press.
        // Only enable when the click lands inside the input box region.
        if x < chat_width && y >= input_start_row {
            state.cursor_mouse_pos = Some((x, y));
        }

        // 仅在左侧，且终端最底部 1 行（状态栏）按下时，进入高度拖动模式。
        // 避免点击输入框（通常会落在倒数第 2 行）时误触发拖动逻辑导致闪烁/焦点异常。
        if x < chat_width && y >= rows.saturating_sub(1) {
            state.input_dragging = true;
            state.input_drag_row = y;
            state.status_line = format!(
                "正在拖动输入区域高度（当前：{} 行）",
                state.input_rows
            );
            return;
        }

        apply_click_focus_and_tab(x, y, cols, rows, state);
    }
}

fn apply_click_focus_and_tab(
    col: u16,
    row: u16,
    cols: u16,
    rows: u16,
    state: &mut TuiState,
) {
    let chat_width = cols.saturating_mul(65) / 100;
    // 右侧面板的鼠标点击：为避免“mouse-down”视觉闪烁，将焦点/Tab 切换延迟到鼠标释放。
    // 左侧（聊天/输入）保持即时响应。
    let defer_to_release = col >= chat_width;

    // 右侧标签栏点击：按列位置切换 tab（工作区/任务/日程）
    if col >= chat_width && row <= 3 {
        let right_x = col.saturating_sub(chat_width);
        let right_w = cols.saturating_sub(chat_width).max(3);
        let inner_w = right_w.saturating_sub(2).max(3);
        let inner_x = right_x.saturating_sub(1).min(inner_w.saturating_sub(1));

        // Tabs 是按内容宽度紧凑排布，不是平均三等分；按标题宽度做命中更准确
        let titles = RightTab::titles();
        let mut cursor: u16 = 0;
        let mut tab_idx: u16 = 2;
        for (i, t) in titles.iter().enumerate() {
            // 每个标签近似渲染为 " <title> "
            let w = (t.width() as u16).saturating_add(2);
            if inner_x >= cursor && inner_x < cursor.saturating_add(w) {
                tab_idx = i as u16;
                break;
            }
            // 标签间分隔符占 1 列
            cursor = cursor.saturating_add(w).saturating_add(1);
        }
        let new_tab = match tab_idx {
            0 => RightTab::Workspace,
            1 => RightTab::Tasks,
            _ => RightTab::Schedule,
        };
        let new_focus = match new_tab {
            RightTab::Workspace => Focus::Workspace,
            RightTab::Tasks => Focus::Right,
            RightTab::Schedule => Focus::Right,
        };

        if defer_to_release {
            state.pending_tab = Some(new_tab);
            state.pending_focus = Some(new_focus);
        } else {
            state.tab = new_tab;
            state.focus = new_focus;
            state.status_line = format!(
                "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                state.status_line.split('：').nth(1).unwrap_or("").trim(),
                focus_name(state.focus)
            );
        }
        return;
    }

    let new_focus = if col < chat_width {
        // 左侧区域再按纵向分成“聊天区 / 输入区”
        let input_start_row = rows.saturating_sub(state.input_rows + 1);
        if row >= input_start_row {
            Focus::ChatInput
        } else {
            Focus::ChatView
        }
    } else {
        // 右侧再细分：workspace 标签下，点内容区聚焦到“工作区”
        let tabs_h: u16 = 3;
        if state.tab == RightTab::Workspace && row > tabs_h {
            Focus::Workspace
        } else {
            Focus::Right
        }
    };

    if new_focus != state.focus {
        if defer_to_release {
            state.pending_focus = Some(new_focus);
        } else {
            state.focus = new_focus;
            if new_focus == Focus::ChatInput {
                // Arm a one-shot cursor override so it appears where the user clicked.
                state.cursor_override = Some((col, row));
            }
            state.status_line = format!(
                "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                state.status_line.split('：').nth(1).unwrap_or("").trim(),
                focus_name(state.focus)
            );
        }
    }

    // If the click lands inside the input area, always reposition the cursor immediately.
    // This is safe for the requested flicker fix (right-panel clicks are deferred).
    if new_focus == Focus::ChatInput && !defer_to_release {
        state.cursor_override = Some((col, row));
    }
}

fn draw_right(f: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);

    let titles: Vec<Line> = RightTab::titles()
        .iter()
        .map(|t| Line::from(Span::raw(*t)))
        .collect();
    let right_focused = state.focus == Focus::Right;
    // Tabs 区：使用角标替代边框线
    let tabs_bg = Color::DarkGray;
    let tabs_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(tabs_bg));
    let tabs = Tabs::new(titles)
        .select(state.tab as usize)
        .block(tabs_block)
        .highlight_style(
            Style::default()
                .fg(right_tab_color(state.tab))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    f.render_widget(tabs, vchunks[0]);
    let tabs_corner_color = if right_focused {
        right_tab_color(state.tab)
    } else {
        Color::DarkGray
    };
    draw_rect_corners(
        f,
        vchunks[0],
        "┏",
        "┓",
        "┗",
        "┛",
        Style::default()
            .fg(tabs_corner_color)
            .add_modifier(Modifier::BOLD),
    );

    match state.tab {
        RightTab::Workspace => {
            let mut lines = Vec::new();
            lines.push(Line::raw(format!("根目录：{}", state.workspace_dir.display())));
            lines.push(Line::raw("快捷键：F2 聚焦 | Enter 打开/进入 | Backspace 上级 | ↑↓ 选择 | r 刷新"));
            lines.push(Line::raw(""));
            for (i, (name, is_dir)) in state.workspace_entries.iter().enumerate().take(200) {
                let prefix = if *is_dir { "[D]" } else { "   " };
                let s = format!("{} {}", prefix, name);
                if i == state.workspace_sel {
                    lines.push(Line::from(Span::styled(s, Style::default().add_modifier(Modifier::REVERSED))));
                } else {
                    lines.push(Line::raw(s));
                }
            }
            let workspace_focused = state.focus == Focus::Workspace;
            let workspace_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Black));
            let w = Paragraph::new(lines)
                .block(workspace_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if workspace_focused {
                Color::Green
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
        RightTab::Tasks => {
            let mut lines = Vec::new();
            lines.push(Line::raw("快捷键：F2 聚焦 | Space 勾选/取消 | ↑↓ 选择 | r 刷新"));
            lines.push(Line::raw(""));
            if state.task_items.is_empty() {
                lines.push(Line::raw("tasks.json 不存在或为空。"));
            } else {
                for (i, (_id, title, done)) in state.task_items.iter().enumerate().take(200) {
                    let s = format!("[{}] {}", if *done { "✓" } else { " " }, title);
                    if state.focus == Focus::Right && i == state.task_sel {
                        lines.push(Line::from(Span::styled(
                            s,
                            Style::default().add_modifier(Modifier::REVERSED),
                        )));
                    } else {
                        lines.push(Line::raw(s));
                    }
                }
            }
            let tasks_focused = state.focus == Focus::Right && state.tab == RightTab::Tasks;
            let tasks_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Blue));
            let w = Paragraph::new(lines)
                .block(tasks_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if tasks_focused {
                Color::Yellow
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
        RightTab::Schedule => {
            let mut lines = Vec::new();
            lines.push(Line::raw(
                "快捷键：F2 聚焦 | t=提醒 e=日程 | Space 完成/取消提醒 | a 新增提醒 | ↑↓ 选择 | r 刷新",
            ));
            lines.push(Line::raw(""));
            let sub_title = if state.schedule_sub == 0 { "提醒" } else { "日程" };
            lines.push(Line::from(Span::styled(
                format!("当前：{}", sub_title),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));

            if state.schedule_sub == 0 {
                if state.reminder_items.is_empty() {
                    lines.push(Line::raw("（无提醒）"));
                } else {
                    for (i, (_id, title, done, due_at)) in state.reminder_items.iter().enumerate().take(50) {
                        let mut s = format!("[{}] {}", if *done { "✓" } else { " " }, title);
                        if due_at.is_some() {
                            s.push_str(" (有到期时间)");
                        }
                        if state.focus == Focus::Right && i == state.reminder_sel {
                            lines.push(Line::from(Span::styled(
                                s,
                                Style::default().add_modifier(Modifier::REVERSED),
                            )));
                        } else {
                            lines.push(Line::raw(s));
                        }
                    }
                }
            } else if state.event_items.is_empty() {
                lines.push(Line::raw("（无日程）"));
            } else {
                for (i, (_id, title, start_at)) in state.event_items.iter().enumerate().take(50) {
                    let s = if start_at.is_empty() {
                        title.clone()
                    } else {
                        format!("{} (有开始时间)", title)
                    };
                    if state.focus == Focus::Right && i == state.event_sel {
                        lines.push(Line::from(Span::styled(
                            s,
                            Style::default().add_modifier(Modifier::REVERSED),
                        )));
                    } else {
                        lines.push(Line::raw(s));
                    }
                }
            }
            let schedule_focused = state.focus == Focus::Right && state.tab == RightTab::Schedule;
            let schedule_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Magenta));
            let w = Paragraph::new(lines)
                .block(schedule_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if schedule_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
    }

    if state.mode == Mode::FileView {
        let block = Block::default()
            .borders(Borders::NONE)
            .padding(Padding::symmetric(1, 1))
            .style(Style::default().bg(Color::DarkGray));
        let title = format!("查看文件（Esc/q 关闭）：{}", state.file_view_title);
        let full = format!("{}\n{}\n", title, state.file_view_content);
        let content = Paragraph::new(full)
            .block(block)
            .wrap(Wrap { trim: false });
        // overlay on right area for simplicity
        f.render_widget(content, vchunks[1]);
        draw_rect_corners(
            f,
            vchunks[1],
            "┏",
            "┓",
            "┗",
            "┛",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
        );
    }
}

