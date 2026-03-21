//! 键盘与鼠标输入（crossterm，与 `CrosstermBackend` 一致）。

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use unicode_width::UnicodeWidthStr;

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::types::{CommandApprovalDecision, Message};

use super::agent::run_agent_turn_tui;
use super::allowlist::save_persistent_allowlist;
use super::state::{
    feed_char_filter_sgr_mouse_leak, Focus, Mode, RightTab, TuiState,
};
use super::status::{set_high_contrast_status_line, set_normal_status_line};
use super::styles::code_themes;
use super::workspace_ops::{
    refresh_schedule, refresh_tasks, refresh_workspace, split_title_due, toggle_reminder_done,
    toggle_task_done, workspace_go_up, workspace_open_or_enter,
};

/// TUI 按键处理所需的 Agent / 通道 / 配置上下文，避免 `handle_key` 参数过长。
pub(super) struct HandleKeyContext<'a> {
    pub agent_running: &'a mut Option<tokio::task::JoinHandle<()>>,
    pub assistant_buf: &'a mut String,
    pub approval_tx: &'a mut Option<mpsc::Sender<CommandApprovalDecision>>,
    pub tx: &'a mpsc::Sender<String>,
    pub sync_tx: mpsc::Sender<Vec<Message>>,
    pub cfg: &'a AgentConfig,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub tools: &'a [crate::types::Tool],
    pub no_stream: bool,
}

pub(super) async fn handle_key(
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
    if key.kind == KeyEventKind::Release {
        return Ok(false);
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    state.cursor_mouse_pos = None;

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
                state.mouse_leak_scratch.clear();
                state.prompt.clear();
                state.prompt_title.clear();
            }
            KeyCode::Enter => {
                if state.prompt_title.starts_with("新增提醒") {
                    let raw = state.prompt.trim();
                    if !raw.is_empty() {
                        let (title, due_at) = split_title_due(raw);
                        let args = if let Some(d) = due_at {
                            serde_json::json!({ "title": title, "due_at": d }).to_string()
                        } else {
                            serde_json::json!({ "title": title }).to_string()
                        };
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
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
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
            set_normal_status_line(state, &cfg.model);
            state.pending_command.clear();
            state.pending_command_args.clear();
        }

        match key.code {
            KeyCode::Esc => {
                state.approve_choice = 0;
            }
            KeyCode::Left => {
                state.approve_choice = state.approve_choice.saturating_sub(1);
            }
            KeyCode::Right => {
                state.approve_choice = (state.approve_choice + 1).min(2);
            }
            KeyCode::Char('1') => state.approve_choice = 0,
            KeyCode::Char('2') => state.approve_choice = 1,
            KeyCode::Char('3') => state.approve_choice = 2,
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
            state.cursor_override = None;
            state.focus = match state.focus {
                Focus::ChatView => Focus::ChatInput,
                Focus::ChatInput => Focus::Workspace,
                Focus::Workspace => Focus::Right,
                Focus::Right => Focus::ChatView,
            };
            set_normal_status_line(state, &cfg.model);
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
            set_high_contrast_status_line(state, &cfg.model);
        }
        KeyCode::PageUp => {
            let step = state.input_rows.max(3) as usize;
            state.chat_follow_tail = false;
            state.chat_first_line = state.chat_first_line.saturating_sub(step);
        }
        KeyCode::PageDown => {
            let step = state.input_rows.max(3) as usize;
            state.chat_first_line = state.chat_first_line.saturating_add(step);
        }
        KeyCode::Tab => {
            state.tab = match state.tab {
                RightTab::Workspace => RightTab::Tasks,
                RightTab::Tasks => RightTab::Schedule,
                RightTab::Schedule => RightTab::Workspace,
            };
            if state.focus == Focus::Workspace && state.tab != RightTab::Workspace {
                state.focus = Focus::Right;
            }
            match state.tab {
                RightTab::Workspace => refresh_workspace(state),
                RightTab::Tasks => refresh_tasks(state),
                RightTab::Schedule => refresh_schedule(state),
            }
        }
        KeyCode::Up => {
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
        KeyCode::Down => {
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
                    RightTab::Tasks | RightTab::Schedule => {}
                }
                return Ok(false);
            }
            if agent_running.is_none() && state.focus == Focus::ChatInput {
                let q = state.input.trim().to_string();
                if !q.is_empty() {
                    state.mouse_leak_scratch.clear();
                    state.cursor_override = None;
                    state.input.clear();
                    state.chat_follow_tail = true;
                    state.messages.push(Message {
                        role: "user".to_string(),
                        content: Some(q),
                        tool_calls: None,
                        name: None,
                        tool_call_id: None,
                    });
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
                    let workspace_is_set = true;
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
            } else if state.focus == Focus::ChatInput {
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
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && state.focus == Focus::ChatInput
            {
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

/// crossterm 鼠标事件（与 `EnableMouseCapture` / `DisableMouseCapture` 配套）。
pub(super) fn handle_crossterm_mouse(
    me: MouseEvent,
    state: &mut TuiState,
    cols: u16,
    rows: u16,
    model: &str,
) {
    let x = me.column;
    let y = me.row;

    match me.kind {
        MouseEventKind::ScrollUp => {
            state.chat_follow_tail = false;
            state.chat_first_line = state.chat_first_line.saturating_sub(3);
        }
        MouseEventKind::ScrollDown => {
            state.chat_first_line = state.chat_first_line.saturating_add(3);
        }
        MouseEventKind::Drag(MouseButton::Left) if state.input_dragging => {
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
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if state.input_dragging {
                state.input_dragging = false;
                state.status_line = format!(
                    "输入区域高度已调整为 {} 行（在底部拖动可再次调整）",
                    state.input_rows
                );
            } else {
                apply_pending_focus_and_tab(state, model);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let chat_width = cols.saturating_mul(65) / 100;
            let input_start_row = rows.saturating_sub(state.input_rows + 1);

            if x < chat_width && y >= input_start_row {
                state.cursor_mouse_pos = Some((x, y));
            }

            if x < chat_width && y >= rows.saturating_sub(1) {
                state.input_dragging = true;
                state.input_drag_row = y;
                state.status_line = format!(
                    "正在拖动输入区域高度（当前：{} 行）",
                    state.input_rows
                );
                return;
            }

            apply_click_focus_and_tab(x, y, cols, rows, state, model);
        }
        _ => {}
    }
}

fn apply_click_focus_and_tab(
    col: u16,
    row: u16,
    cols: u16,
    rows: u16,
    state: &mut TuiState,
    model: &str,
) {
    let chat_width = cols.saturating_mul(65) / 100;
    let defer_to_release = col >= chat_width;

    if col >= chat_width && row <= 3 {
        let right_x = col.saturating_sub(chat_width);
        let right_w = cols.saturating_sub(chat_width).max(3);
        let inner_w = right_w.saturating_sub(2).max(3);
        let inner_x = right_x.saturating_sub(1).min(inner_w.saturating_sub(1));

        let titles = RightTab::titles();
        let mut cursor: u16 = 0;
        let mut tab_idx: u16 = 2;
        for (i, t) in titles.iter().enumerate() {
            let w = (t.width() as u16).saturating_add(2);
            if inner_x >= cursor && inner_x < cursor.saturating_add(w) {
                tab_idx = i as u16;
                break;
            }
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
            set_normal_status_line(state, model);
        }
        return;
    }

    let new_focus = if col < chat_width {
        let input_start_row = rows.saturating_sub(state.input_rows + 1);
        if row >= input_start_row {
            Focus::ChatInput
        } else {
            Focus::ChatView
        }
    } else {
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
                state.cursor_override = Some((col, row));
            }
            set_normal_status_line(state, model);
        }
    }

    if new_focus == Focus::ChatInput && !defer_to_release {
        state.cursor_override = Some((col, row));
    }
}

fn apply_pending_focus_and_tab(state: &mut TuiState, model: &str) {
    let mut changed = false;

    if let Some(tab) = state.pending_tab.take() {
        if state.tab != tab {
            state.tab = tab;
            changed = true;
            match state.tab {
                RightTab::Workspace => refresh_workspace(state),
                RightTab::Tasks => refresh_tasks(state),
                RightTab::Schedule => refresh_schedule(state),
            }
        }
    }
    if let Some(focus) = state.pending_focus.take()
        && state.focus != focus
    {
        state.focus = focus;
        changed = true;
    }

    if changed {
        set_normal_status_line(state, model);
    }
}
