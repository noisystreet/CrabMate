use crate::config::AgentConfig;
use crate::run_agent_turn;
use crate::types::Message;
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        EnableMouseCapture, DisableMouseCapture,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Terminal,
};
use std::io;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tui_markdown::{from_str_with_options as markdown_to_text, Options, StyleSheet};
use unicodeit::replace as latex_to_unicode;
use unicode_width::UnicodeWidthStr;
use ratatui::layout::Margin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Chat,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    FileView,
    Prompt,
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

struct TuiState {
    // chat
    messages: Vec<Message>,
    input: String,
    prompt: String,
    prompt_title: String,
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
        status_line: format!(
            "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：聊天）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
            cfg.model
        ),
        tool_running: false,
        focus: Focus::Chat,
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
    };
    refresh_workspace(&mut state);
    refresh_tasks(&mut state);
    refresh_schedule(&mut state);

    // terminal init
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // agent output channel
    let (tx, mut rx) = mpsc::channel::<String>(2048);
    let mut agent_running: Option<tokio::task::JoinHandle<()>> = None;
    let mut assistant_buf = String::new();

    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        // pump agent stream into UI state
        while let Ok(s) = rx.try_recv() {
            if s == r#"{"tool_running":true}"# {
                state.tool_running = true;
                state.status_line = "工具运行中…".to_string();
                continue;
            }
            if s == r#"{"tool_running":false}"# {
                state.tool_running = false;
                if state.status_line == "工具运行中…" {
                    state.status_line = format!(
                        "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                        cfg.model,
                        if state.focus == Focus::Chat { "聊天" } else { "右侧" }
                    );
                }
                continue;
            }
            if s == r#"{"workspace_changed":true}"# {
                refresh_workspace(&mut state);
                refresh_tasks(&mut state);
                refresh_schedule(&mut state);
                continue;
            }
            if s.starts_with("{\"error\"") {
                // backend error JSON string from stream_chat wrapper
                assistant_buf.push_str("\n");
                assistant_buf.push_str(&s);
                continue;
            }
            // normal content delta
            assistant_buf.push_str(&s);
            upsert_assistant_message(&mut state.messages, &assistant_buf);
        }

        // draw
        terminal.draw(|f| draw_ui(f, &state))?;

        // finish agent task if done
        if let Some(handle) = agent_running.as_ref() {
            if handle.is_finished() {
                agent_running = None;
                state.status_line = format!(
                    "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                    cfg.model,
                    if state.focus == Focus::Chat { "聊天" } else { "右侧" }
                );
            }
        }

        // input events
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(
                        key,
                        &mut state,
                        &mut agent_running,
                        &mut assistant_buf,
                        &tx,
                        cfg,
                        client,
                        api_key,
                        tools,
                        no_stream,
                    )
                    .await?
                    {
                        break;
                    }
                }
                Event::Mouse(m) => {
                    handle_mouse(m, &mut state);
                }
                Event::Resize(_, _) => {
                    // ignore, layout will be recomputed on next draw
                }
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => {
                    // currently no-op
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    // restore terminal
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(DisableMouseCapture)?;
    stdout.execute(LeaveAlternateScreen)?;
    Ok(())
}

async fn handle_key(
    key: KeyEvent,
    state: &mut TuiState,
    agent_running: &mut Option<tokio::task::JoinHandle<()>>,
    assistant_buf: &mut String,
    tx: &mpsc::Sender<String>,
    cfg: &AgentConfig,
    client: &reqwest::Client,
    api_key: &str,
    tools: &[crate::types::Tool],
    no_stream: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    // exit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    // modal / prompt
    if state.mode == Mode::FileView {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                state.mode = Mode::Normal;
                state.file_view_title.clear();
                state.file_view_content.clear();
            }
            _ => {}
        }
        return Ok(false);
    }
    if state.mode == Mode::Prompt {
        match key.code {
            KeyCode::Esc => {
                state.mode = Mode::Normal;
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
                state.prompt.clear();
                state.prompt_title.clear();
            }
            KeyCode::Backspace => {
                state.prompt.pop();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
                    state.prompt.push(ch);
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    // 全局帮助弹窗优先处理（除 Ctrl+C 外）
    if state.show_help {
        match key.code {
            KeyCode::F(1) | KeyCode::Esc => {
                state.show_help = false;
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::F(1) => {
            state.show_help = !state.show_help;
        }
        KeyCode::F(2) => {
            state.focus = if state.focus == Focus::Chat { Focus::Right } else { Focus::Chat };
            state.status_line = format!(
                "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                cfg.model,
                if state.focus == Focus::Chat { "聊天" } else { "右侧" }
            );
        }
        KeyCode::F(3) => {
            state.code_theme_idx = (state.code_theme_idx + 1) % code_themes().len();
            state.status_line = format!("代码主题：{}（F3 切换）", code_themes()[state.code_theme_idx]);
        }
        KeyCode::F(4) => {
            state.md_style = if state.md_style == 0 { 1 } else { 0 };
            state.status_line = format!(
                "Markdown样式：{}（F4 切换）",
                if state.md_style == 0 { "dark" } else { "light" }
            );
        }
        KeyCode::F(5) => {
            state.high_contrast = !state.high_contrast;
            state.status_line = format!(
                "高对比度：{}（F5 切换）  |  模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                if state.high_contrast { "开" } else { "关" },
                cfg.model,
                if state.focus == Focus::Chat { "聊天" } else { "右侧" }
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
            // refresh view data on tab switch
            match state.tab {
                RightTab::Workspace => refresh_workspace(state),
                RightTab::Tasks => refresh_tasks(state),
                RightTab::Schedule => refresh_schedule(state),
            }
        }
        KeyCode::Up => {
            if state.focus == Focus::Right {
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
        KeyCode::Down => {
            if state.focus == Focus::Right {
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
            if state.focus == Focus::Right {
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
            if agent_running.is_none() && state.focus == Focus::Chat {
                let q = state.input.trim().to_string();
                if !q.is_empty() {
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
                    let _no_stream = no_stream;
                    let cfg = cfg.clone();
                    let client = client.clone();
                    let api_key = api_key.to_string();
                    let tools = tools.to_vec();
                    *agent_running = Some(tokio::spawn(async move {
                        let out = Some(&tx2);
                        let _ = run_agent_turn(
                            &client,
                            &api_key,
                            &cfg,
                            &tools,
                            &mut messages,
                            out,
                            &work_dir,
                            workspace_is_set,
                            false,
                        )
                        .await;
                        // 结束标记交给上层通过 join handle 检测
                        let _ = tx2.send(r#"{"tool_running":false}"#.to_string()).await;
                    }));
                }
            }
        }
        KeyCode::Backspace => {
            if state.focus == Focus::Right && state.tab == RightTab::Workspace {
                workspace_go_up(state);
            } else if state.focus == Focus::Chat {
                state.input.pop();
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
                state.input.push(' ');
            }
        }
        KeyCode::Char('a') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule && state.schedule_sub == 0 {
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
            if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
                if state.focus == Focus::Chat {
                    state.input.push(ch);
                }
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
    for m in state.messages.iter().filter(|m| m.role != "system") {
        let role = if m.role == "user" { "你" } else { "Agent" };
        let raw = m.content.as_deref().unwrap_or("");
        // 轻量公式渲染：将常见 LaTeX 命令替换为 Unicode 符号，便于在终端阅读。
        // 为了满足 ratatui Text 的生命周期约束，这里将结果提升为 'static。
        let rendered_owned = latex_to_unicode(raw);
        let rendered: &'static str = Box::leak(rendered_owned.into_boxed_str());
        lines.push(Line::from(Span::styled(
            format!("{}:", role),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if m.role == "assistant" {
            // 使用 tui-markdown 渲染：链接会包含 URL（便于复制/终端自动识别点击）。
            let theme = code_themes()[state.code_theme_idx];
            let text = match (state.md_style, state.high_contrast) {
                (0, false) => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(&rendered, &options)
                }
                (0, true) => {
                    let options = Options::new(HighContrastDarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(&rendered, &options)
                }
                (1, false) => {
                    let options = Options::new(LightStyleSheet).with_code_theme(theme);
                    markdown_to_text(&rendered, &options)
                }
                (1, true) => {
                    let options = Options::new(HighContrastLightStyleSheet).with_code_theme(theme);
                    markdown_to_text(&rendered, &options)
                }
                _ => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(&rendered, &options)
                }
            };
            lines.extend(text.lines.into_iter());
        } else {
            // user message: keep raw to avoid surprising formatting
            for l in rendered.lines() {
                lines.push(Line::raw(l));
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
    let chat_focused = state.focus == Focus::Chat;
    let chat_block = if chat_focused {
        Block::default()
            .borders(Borders::ALL)
            .title("对话")
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title("对话")
            .border_style(Style::default().fg(Color::DarkGray))
            .title_style(Style::default().fg(Color::DarkGray))
    };
    let chat = Paragraph::new(lines)
        .block(chat_block)
        .wrap(Wrap { trim: false });
    f.render_widget(chat, vchunks[0]);

    let input_title = if state.mode == Mode::Prompt {
        state.prompt_title.as_str()
    } else if state.focus == Focus::Chat {
        "输入（Enter 发送 | F2 切到右侧 | 底部拖动调节高度）"
    } else {
        "输入（F2 切回聊天 | 底部拖动调节高度）"
    };
    let input_text = if state.mode == Mode::Prompt {
        state.prompt.as_str()
    } else {
        state.input.as_str()
    };
    let input_focused = state.mode == Mode::Prompt || state.focus == Focus::Chat;
    let input_block = if input_focused {
        Block::default()
            .borders(Borders::ALL)
            .title(input_title)
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(input_title)
            .border_style(Style::default().fg(Color::DarkGray))
            .title_style(Style::default().fg(Color::DarkGray))
    };
    let input = Paragraph::new(input_text)
        .block(input_block)
        .style(if input_focused { Style::default() } else { Style::default().fg(Color::Gray) })
        .wrap(Wrap { trim: false });
    f.render_widget(input, vchunks[1]);

    // 显示终端光标（由终端负责闪烁）：聚焦时把光标放到输入内容末尾
    if input_focused {
        let inner = vchunks[1].inner(Margin { vertical: 1, horizontal: 1 });
        if inner.width > 0 && inner.height > 0 {
            let lines: Vec<&str> = input_text.split('\n').collect();
            let line_idx = lines.len().saturating_sub(1);
            let last = lines.get(line_idx).copied().unwrap_or("");
            let x_off = last.width() as u16;
            let x = inner.x.saturating_add(x_off).min(inner.x + inner.width.saturating_sub(1));
            let y = inner.y.saturating_add(line_idx as u16).min(inner.y + inner.height.saturating_sub(1));
            f.set_cursor_position((x, y));
        }
    }

    let status_block = if state.focus == Focus::Chat {
        Block::default()
            .borders(Borders::ALL)
            .title("状态（聊天聚焦）")
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title("状态（右侧聚焦）")
            .border_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
            .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
    };
    let status = Paragraph::new(state.status_line.as_str()).block(status_block);
    f.render_widget(status, vchunks[2]);
}

fn handle_mouse(me: MouseEvent, state: &mut TuiState) {
    match me.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // 在终端底部若干行内按下，进入“拖动输入区域高度”模式
            if let Ok((_cols, rows)) = crossterm::terminal::size() {
                if me.row >= rows.saturating_sub(6) {
                    state.input_dragging = true;
                    state.input_drag_row = me.row;
                    state.status_line = format!(
                        "正在拖动输入区域高度（当前：{} 行）",
                        state.input_rows
                    );
                    return;
                }
            }
            // 普通点击：根据横向位置切换焦点（左侧=聊天，右侧=面板）
            if let Ok((cols, _rows)) = crossterm::terminal::size() {
                // 与布局保持一致：左 65% 聊天，右 35% 面板
                let chat_width = (cols as f32 * 0.65).round() as u16;
                let new_focus = if me.column < chat_width {
                    Focus::Chat
                } else {
                    Focus::Right
                };
                if new_focus != state.focus {
                    state.focus = new_focus;
                    state.status_line = format!(
                        "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
                        state.status_line.split('：').nth(1).unwrap_or("").trim(),
                        if state.focus == Focus::Chat { "聊天" } else { "右侧" }
                    );
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if state.input_dragging {
                let prev = state.input_drag_row;
                let cur = me.row;
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
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.input_dragging = false;
            state.status_line = format!(
                "输入区域高度已调整为 {} 行（在底部拖动可再次调整）",
                state.input_rows
            );
        }
        MouseEventKind::ScrollUp => {
            // 鼠标滚轮向上：聊天区域向下滚（更符合多数终端/编辑器习惯）
            state.chat_scroll -= 3;
            if state.chat_scroll < 0 {
                state.chat_scroll = 0;
            }
        }
        MouseEventKind::ScrollDown => {
            // 鼠标滚轮向下：聊天区域向上滚
            state.chat_scroll += 3;
        }
        _ => {}
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
    let tabs_block = if right_focused {
        Block::default()
            .borders(Borders::ALL)
            .title("面板（Tab 切换）")
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title("面板（Tab 切换）")
            .border_style(Style::default().fg(Color::DarkGray))
            .title_style(Style::default().fg(Color::DarkGray))
    };
    let tabs = Tabs::new(titles)
        .select(state.tab as usize)
        .block(tabs_block)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(tabs, vchunks[0]);

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
            let w = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("工作区（↑↓ 选择）"))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
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
            let w = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("任务"))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
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
            } else {
                if state.event_items.is_empty() {
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
            }
            let w = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("日程/提醒"))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
        }
    }

    if state.mode == Mode::FileView {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!("查看文件（Esc/q 关闭）：{}", state.file_view_title));
        let content = Paragraph::new(state.file_view_content.as_str())
            .block(block)
            .wrap(Wrap { trim: false });
        // overlay on right area for simplicity
        f.render_widget(content, vchunks[1]);
    }
}

