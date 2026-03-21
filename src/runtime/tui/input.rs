//! 键盘与鼠标输入（crossterm，与 `CrosstermBackend` 一致）。

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use unicode_width::UnicodeWidthStr;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::health::{build_health_report, format_health_report_terminal};
use crate::types::{CommandApprovalDecision, LLM_CANCELLED_ERROR, Message};

use super::agent::run_agent_turn_tui;
use super::allowlist::save_persistent_allowlist;
use super::chat_nav;
use super::chat_session;
use super::clipboard;
use super::draw;
use super::edit_history;
use super::state::{
    Focus, Mode, ModelPhase, RightTab, TuiState, collect_feed_chars_after_sgr_filter,
};
use super::status::{set_high_contrast_status_line, set_normal_status_line};
use super::styles::code_themes;
use super::text_input;
use super::workspace_ops::{
    refresh_schedule, refresh_tasks, refresh_workspace, split_title_due, toggle_reminder_done,
    toggle_task_done, workspace_go_up, workspace_open_or_enter,
};

fn insert_tab_chat(state: &mut TuiState) {
    if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
        edit_history::push_input_undo(state);
        text_input::insert_at_cursor(&mut state.input, &mut state.input_cursor, '\t');
    }
}

fn insert_tab_prompt(state: &mut TuiState) {
    edit_history::push_prompt_undo(state);
    text_input::insert_at_cursor(&mut state.prompt, &mut state.prompt_cursor, '\t');
}

fn paste_chat(state: &mut TuiState) {
    if state.focus != Focus::ChatInput || state.mode != Mode::Normal {
        return;
    }
    if let Some(t) = clipboard::try_clipboard_text() {
        edit_history::push_input_undo(state);
        text_input::insert_str_at_cursor(&mut state.input, &mut state.input_cursor, &t);
    }
}

fn paste_prompt(state: &mut TuiState) {
    if let Some(t) = clipboard::try_clipboard_text() {
        edit_history::push_prompt_undo(state);
        text_input::insert_str_at_cursor(&mut state.prompt, &mut state.prompt_cursor, &t);
    }
}

/// 焦点在聊天输入且无 Ctrl/Alt 时写入输入缓冲；成功则返回 `true`。
///
/// 用于在 `match key.code` 里先于通用 `Char(_)` 匹配的专用字母键分支，避免条件不成立时把字符吞掉。
fn try_feed_chat_input(key: &KeyEvent, state: &mut TuiState, ch: char) -> bool {
    if state.focus != Focus::ChatInput {
        return false;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
        return false;
    }
    let chars = collect_feed_chars_after_sgr_filter(&mut state.mouse_leak_scratch, ch);
    if !chars.is_empty() {
        edit_history::push_input_undo(state);
    }
    for c in chars {
        text_input::insert_at_cursor(&mut state.input, &mut state.input_cursor, c);
    }
    true
}

/// TUI 按键处理所需的 Agent / 通道 / 配置上下文，避免 `handle_key` 参数过长。
pub(super) struct HandleKeyContext<'a> {
    pub agent_running: &'a mut Option<tokio::task::JoinHandle<()>>,
    pub assistant_buf: &'a mut String,
    pub approval_tx: &'a mut Option<mpsc::Sender<CommandApprovalDecision>>,
    pub tx: &'a mpsc::Sender<String>,
    pub sync_tx: mpsc::Sender<Vec<Message>>,
    pub agent_cancel: Arc<AtomicBool>,
    pub cfg: &'a AgentConfig,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub tools: &'a [crate::types::Tool],
    pub no_stream: bool,
    pub term_cols: u16,
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
        agent_cancel,
        cfg,
        client,
        api_key,
        tools,
        no_stream,
        term_cols,
    } = ctx;
    if key.kind == KeyEventKind::Release {
        return Ok(false);
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    if agent_running.is_some() {
        let g = matches!(key.code, KeyCode::Char('g') | KeyCode::Char('G'));
        if g && key.modifiers.contains(KeyModifiers::CONTROL) {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                if let Some(h) = agent_running.take() {
                    h.abort();
                }
                *approval_tx = None;
                agent_cancel.store(false, Ordering::SeqCst);
                state.tool_running = false;
                state.tool_running_clear_pending = false;
                state.model_phase = ModelPhase::Idle;
                set_normal_status_line(state, &cfg.model);
                state.status_line = "已强制中止本轮（工具执行中可能无法立刻停下）".to_string();
            } else {
                agent_cancel.store(true, Ordering::SeqCst);
                state.status_line = "正在停止生成…".to_string();
            }
            return Ok(false);
        }
    }

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
        let mw = text_input::left_column_inner_text_width(term_cols);
        match key.code {
            KeyCode::Esc => {
                state.mode = Mode::Normal;
                state.mouse_leak_scratch.clear();
                state.prompt.clear();
                state.prompt_cursor = 0;
                edit_history::clear_prompt_history(state);
                state.prompt_title.clear();
            }
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    edit_history::push_prompt_undo(state);
                    text_input::insert_at_cursor(&mut state.prompt, &mut state.prompt_cursor, '\n');
                } else {
                    if state
                        .prompt_title
                        .starts_with(chat_nav::PROMPT_TITLE_SEARCH)
                    {
                        let q = state.prompt.clone();
                        chat_nav::apply_chat_search(state, &q, term_cols);
                    } else if state.prompt_title.starts_with(chat_nav::PROMPT_TITLE_JUMP) {
                        let raw = state.prompt.clone();
                        let _ = chat_nav::apply_jump_to_message(state, &raw, term_cols);
                    } else if state.prompt_title.starts_with("新增提醒") {
                        let raw = state.prompt.trim();
                        if !raw.is_empty() {
                            let (title, due_at) = split_title_due(raw);
                            let args = if let Some(d) = due_at {
                                serde_json::json!({ "title": title, "due_at": d }).to_string()
                            } else {
                                serde_json::json!({ "title": title }).to_string()
                            };
                            let tool_ctx = crate::tools::tool_context_for(
                                cfg,
                                &cfg.allowed_commands,
                                &state.workspace_dir,
                            );
                            let _ = crate::tools::run_tool("add_reminder", &args, &tool_ctx);
                            refresh_schedule(state);
                        }
                    }
                    state.mode = Mode::Normal;
                    state.mouse_leak_scratch.clear();
                    state.prompt.clear();
                    state.prompt_cursor = 0;
                    edit_history::clear_prompt_history(state);
                    state.prompt_title.clear();
                }
            }
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    insert_tab_prompt(state);
                }
            }
            KeyCode::Char('\t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                insert_tab_prompt(state);
            }
            KeyCode::Char('z')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                let _ = edit_history::prompt_undo(state);
            }
            KeyCode::Char('Z')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                let _ = edit_history::prompt_redo(state);
            }
            KeyCode::Char('y') | KeyCode::Char('Y')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let _ = edit_history::prompt_redo(state);
            }
            KeyCode::Char('v') | KeyCode::Char('V')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                paste_prompt(state);
            }
            KeyCode::Char('i') | KeyCode::Char('I')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                insert_tab_prompt(state);
            }
            KeyCode::Backspace => {
                if !state.mouse_leak_scratch.is_empty() {
                    state.mouse_leak_scratch.pop();
                } else {
                    edit_history::push_prompt_undo(state);
                    text_input::delete_before_cursor(&mut state.prompt, &mut state.prompt_cursor);
                }
            }
            KeyCode::Delete => {
                edit_history::push_prompt_undo(state);
                text_input::delete_after_cursor(&mut state.prompt, &mut state.prompt_cursor);
            }
            KeyCode::Left => {
                text_input::cursor_step_left(&state.prompt, &mut state.prompt_cursor);
            }
            KeyCode::Right => {
                text_input::cursor_step_right(&state.prompt, &mut state.prompt_cursor);
            }
            KeyCode::Up => {
                text_input::cursor_move_vertical(&state.prompt, &mut state.prompt_cursor, mw, -1);
            }
            KeyCode::Down => {
                text_input::cursor_move_vertical(&state.prompt, &mut state.prompt_cursor, mw, 1);
            }
            KeyCode::Home => {
                text_input::home_logical_line(&state.prompt, &mut state.prompt_cursor);
            }
            KeyCode::End => {
                text_input::end_logical_line(&state.prompt, &mut state.prompt_cursor);
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    let chars =
                        collect_feed_chars_after_sgr_filter(&mut state.mouse_leak_scratch, ch);
                    if !chars.is_empty() {
                        edit_history::push_prompt_undo(state);
                    }
                    for c in chars {
                        text_input::insert_at_cursor(
                            &mut state.prompt,
                            &mut state.prompt_cursor,
                            c,
                        );
                    }
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
                let key_to_store = state
                    .pending_approval_allowlist_key
                    .take()
                    .unwrap_or_else(|| state.pending_command.trim().to_lowercase());
                if !key_to_store.is_empty() {
                    state.persistent_command_allowlist.insert(key_to_store);
                    save_persistent_allowlist(
                        &state.allowlist_file,
                        &state.persistent_command_allowlist,
                    );
                }
            } else {
                state.pending_approval_allowlist_key = None;
            }
            if let Some(ch) = approval_tx.as_ref() {
                let _ = ch.send(decision).await;
            }
            state.mode = Mode::Normal;
            state.model_phase = ModelPhase::Thinking;
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
                submit_approval(state, approval_tx, cfg, CommandApprovalDecision::Deny).await;
            }
            KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'o') => {
                submit_approval(state, approval_tx, cfg, CommandApprovalDecision::AllowOnce).await;
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

    if state.show_health {
        match key.code {
            KeyCode::F(10) | KeyCode::Esc => {
                state.show_health = false;
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
            if state.show_help {
                state.show_health = false;
            }
        }
        KeyCode::F(2) => {
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
            // 底栏暂不提示代码主题切换（F3 仍生效）
            set_normal_status_line(state, &cfg.model);
        }
        KeyCode::F(4) => {
            state.md_style = if state.md_style == 0 { 1 } else { 0 };
            state.status_line = format!(
                "Markdown样式：{}",
                if state.md_style == 0 { "dark" } else { "light" }
            );
        }
        KeyCode::F(5) => {
            state.high_contrast = !state.high_contrast;
            set_high_contrast_status_line(state, &cfg.model);
        }
        KeyCode::F(6) if state.mode == Mode::Normal => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                chat_nav::search_next(state, -1);
            } else if state.chat_search_matches.is_empty() {
                state.mouse_leak_scratch.clear();
                state.mode = Mode::Prompt;
                state.prompt.clear();
                state.prompt_cursor = 0;
                edit_history::clear_prompt_history(state);
                state.prompt_title = format!("{}：输入关键词 Enter", chat_nav::PROMPT_TITLE_SEARCH);
            } else {
                chat_nav::search_next(state, 1);
            }
        }
        KeyCode::F(7) if state.mode == Mode::Normal => {
            state.mouse_leak_scratch.clear();
            state.mode = Mode::Prompt;
            state.prompt.clear();
            state.prompt_cursor = 0;
            edit_history::clear_prompt_history(state);
            state.prompt_title = format!(
                "{}：从 1 起（不含系统提示），Enter",
                chat_nav::PROMPT_TITLE_JUMP
            );
        }
        KeyCode::F(8) if state.mode == Mode::Normal => {
            match chat_session::export_json(&state.workspace_dir, &state.messages) {
                Ok(p) => {
                    state.status_line = format!("已导出 JSON：{}", p.display());
                }
                Err(e) => {
                    state.status_line = format!("导出失败：{e}");
                }
            }
        }
        KeyCode::F(9) if state.mode == Mode::Normal => {
            match chat_session::export_markdown(&state.workspace_dir, &state.messages) {
                Ok(p) => {
                    state.status_line = format!("已导出 Markdown：{}", p.display());
                }
                Err(e) => {
                    state.status_line = format!("导出失败：{e}");
                }
            }
        }
        KeyCode::F(10) if state.mode == Mode::Normal => {
            state.show_help = false;
            let report = build_health_report(&state.workspace_dir, api_key, true).await;
            state.health_text = format_health_report_terminal(&report);
            state.show_health = true;
            set_normal_status_line(state, &cfg.model);
        }
        KeyCode::Char('f') | KeyCode::Char('F')
            if key.modifiers.contains(KeyModifiers::CONTROL) && state.mode == Mode::Normal =>
        {
            state.mouse_leak_scratch.clear();
            state.mode = Mode::Prompt;
            state.prompt.clear();
            state.prompt_cursor = 0;
            edit_history::clear_prompt_history(state);
            state.prompt_title = format!("{}：输入关键词 Enter", chat_nav::PROMPT_TITLE_SEARCH);
        }
        KeyCode::Char('z')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                let _ = edit_history::input_undo(state);
            }
        }
        KeyCode::Char('Z')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                let _ = edit_history::input_redo(state);
            }
        }
        KeyCode::Char('y') | KeyCode::Char('Y')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                let _ = edit_history::input_redo(state);
            }
        }
        KeyCode::Char('v') | KeyCode::Char('V')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            paste_chat(state);
        }
        KeyCode::Char('i') | KeyCode::Char('I')
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            insert_tab_chat(state);
        }
        KeyCode::Char('\t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_tab_chat(state);
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
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                insert_tab_chat(state);
            } else {
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
        }
        KeyCode::Up => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                let mw = text_input::left_column_inner_text_width(term_cols);
                text_input::cursor_move_vertical(&state.input, &mut state.input_cursor, mw, -1);
            } else if state.focus == Focus::Right || state.focus == Focus::Workspace {
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
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                let mw = text_input::left_column_inner_text_width(term_cols);
                text_input::cursor_move_vertical(&state.input, &mut state.input_cursor, mw, 1);
            } else if state.focus == Focus::Right || state.focus == Focus::Workspace {
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
                            state.event_sel =
                                (state.event_sel + 1).min(state.event_items.len() - 1);
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            if !try_feed_chat_input(&key, state, 'r') {
                refresh_workspace(state);
                refresh_tasks(state);
                refresh_schedule(state);
            }
        }
        KeyCode::Enter => {
            if agent_running.is_none()
                && state.focus == Focus::ChatInput
                && state.mode == Mode::Normal
                && key.modifiers.contains(KeyModifiers::SHIFT)
            {
                state.mouse_leak_scratch.clear();
                edit_history::push_input_undo(state);
                text_input::insert_at_cursor(&mut state.input, &mut state.input_cursor, '\n');
            } else if state.focus == Focus::Right || state.focus == Focus::Workspace {
                match state.tab {
                    RightTab::Workspace => {
                        workspace_open_or_enter(state);
                    }
                    RightTab::Tasks | RightTab::Schedule => {}
                }
                return Ok(false);
            } else if agent_running.is_none()
                && state.focus == Focus::ChatInput
                && state.mode == Mode::Normal
            {
                let q = state.input.trim().to_string();
                if !q.is_empty() {
                    state.mouse_leak_scratch.clear();
                    state.input.clear();
                    state.input_cursor = 0;
                    edit_history::clear_input_history(state);
                    state.chat_follow_tail = true;
                    state.chat_search_matches.clear();
                    state.chat_search_active_idx = 0;
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
                    state.model_phase = ModelPhase::Thinking;
                    set_normal_status_line(state, &cfg.model);
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
                    agent_cancel.store(false, Ordering::SeqCst);
                    let cancel_arc = agent_cancel.clone();
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
                            Some(cancel_arc.as_ref()),
                        )
                        .await;
                        let user_cancelled =
                            matches!(&res, Err(e) if format!("{e}") == LLM_CANCELLED_ERROR);
                        if !user_cancelled {
                            let _ = sync_tx2.send(messages).await;
                        }
                        if let Err(e) = res
                            && format!("{e}") != LLM_CANCELLED_ERROR
                        {
                            let _ = tx2
                                .send(crate::sse::encode_message(crate::sse::SsePayload::Error(
                                    crate::sse::SseErrorBody {
                                        error: e.to_string(),
                                        code: Some("AGENT_TURN".to_string()),
                                    },
                                )))
                                .await;
                        }
                        let _ = tx2
                            .send(crate::sse::encode_message(
                                crate::sse::SsePayload::ToolRunning {
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
                if !state.mouse_leak_scratch.is_empty() {
                    state.mouse_leak_scratch.pop();
                } else {
                    edit_history::push_input_undo(state);
                    text_input::delete_before_cursor(&mut state.input, &mut state.input_cursor);
                }
            }
        }
        KeyCode::Delete => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                edit_history::push_input_undo(state);
                text_input::delete_after_cursor(&mut state.input, &mut state.input_cursor);
            }
        }
        KeyCode::Left => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                text_input::cursor_step_left(&state.input, &mut state.input_cursor);
            }
        }
        KeyCode::Right => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                text_input::cursor_step_right(&state.input, &mut state.input_cursor);
            }
        }
        KeyCode::Home => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                text_input::home_logical_line(&state.input, &mut state.input_cursor);
            }
        }
        KeyCode::End => {
            if state.focus == Focus::ChatInput && state.mode == Mode::Normal {
                text_input::end_logical_line(&state.input, &mut state.input_cursor);
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
                let chars = collect_feed_chars_after_sgr_filter(&mut state.mouse_leak_scratch, ' ');
                if !chars.is_empty() {
                    edit_history::push_input_undo(state);
                }
                for c in chars {
                    text_input::insert_at_cursor(&mut state.input, &mut state.input_cursor, c);
                }
            }
        }
        KeyCode::Char('a') => {
            if state.focus == Focus::Right
                && state.tab == RightTab::Schedule
                && state.schedule_sub == 0
            {
                state.mouse_leak_scratch.clear();
                state.mode = Mode::Prompt;
                state.prompt.clear();
                state.prompt_cursor = 0;
                edit_history::clear_prompt_history(state);
                state.prompt_title =
                    "新增提醒：输入「标题 @ 2026-03-20 09:00」（@ 后可省略）".to_string();
            } else {
                let _ = try_feed_chat_input(&key, state, 'a');
            }
        }
        KeyCode::Char('e') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule {
                state.schedule_sub = 1;
            } else {
                let _ = try_feed_chat_input(&key, state, 'e');
            }
        }
        KeyCode::Char('t') => {
            if state.focus == Focus::Right && state.tab == RightTab::Schedule {
                state.schedule_sub = 0;
            } else {
                let _ = try_feed_chat_input(&key, state, 't');
            }
        }
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && state.focus == Focus::ChatInput
            {
                let chars = collect_feed_chars_after_sgr_filter(&mut state.mouse_leak_scratch, ch);
                if !chars.is_empty() {
                    edit_history::push_input_undo(state);
                }
                for c in chars {
                    text_input::insert_at_cursor(&mut state.input, &mut state.input_cursor, c);
                }
            }
        }
        _ => {}
    }

    Ok(false)
}

/// crossterm 鼠标事件（与 `EnableMouseCapture` / `DisableMouseCapture` 配套）。
/// 返回是否改变了需要重绘的 UI 状态（用于主循环避免无意义的 `draw`）。
pub(super) fn handle_crossterm_mouse(
    me: MouseEvent,
    state: &mut TuiState,
    cols: u16,
    rows: u16,
    model: &str,
) -> bool {
    let x = me.column;
    let y = me.row;

    match me.kind {
        MouseEventKind::ScrollUp => {
            state.chat_follow_tail = false;
            state.chat_first_line = state.chat_first_line.saturating_sub(3);
            true
        }
        MouseEventKind::ScrollDown => {
            state.chat_first_line = state.chat_first_line.saturating_add(3);
            true
        }
        MouseEventKind::Drag(MouseButton::Left) if state.input_dragging => {
            let prev = state.input_drag_row;
            let cur = y;
            if cur != prev {
                let delta = prev as i16 - cur as i16;
                let new_rows = (state.input_rows as i16 + delta).clamp(3, 12) as u16;
                state.input_rows = new_rows;
                state.input_drag_row = cur;
                state.status_line =
                    format!("正在拖动输入区域高度（当前：{} 行）", state.input_rows);
                true
            } else {
                false
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if state.input_dragging {
                state.input_dragging = false;
                state.status_line = format!("输入区域高度已调整为 {} 行", state.input_rows);
                true
            } else {
                apply_pending_focus_and_tab(state, model)
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let chat_width = cols.saturating_mul(65) / 100;
            let below_input = super::draw::LEFT_COLUMN_ROWS_BELOW_INPUT;
            let input_start_row = rows.saturating_sub(state.input_rows.saturating_add(below_input));

            // 输入区与状态栏之间的横线：拖动调整输入区高度（勿点在状态文字行上）
            if x < chat_width && y == rows.saturating_sub(below_input) {
                state.input_dragging = true;
                state.input_drag_row = y;
                state.status_line =
                    format!("正在拖动输入区域高度（当前：{} 行）", state.input_rows);
                return true;
            }

            let mut changed = false;
            if x < chat_width && y >= input_start_row && y < rows.saturating_sub(below_input) {
                let inner = draw::chat_input_text_inner(cols, rows, state.input_rows);
                if x >= inner.x
                    && x < inner.x.saturating_add(inner.width)
                    && y >= inner.y
                    && y < inner.y.saturating_add(inner.height)
                {
                    let mw = inner.width.max(1) as usize;
                    let rel_x = x.saturating_sub(inner.x);
                    let rel_y = y.saturating_sub(inner.y);
                    if state.mode == Mode::Prompt {
                        state.prompt_cursor =
                            text_input::byte_index_from_mouse_cell(&state.prompt, mw, rel_x, rel_y);
                    } else {
                        state.input_cursor =
                            text_input::byte_index_from_mouse_cell(&state.input, mw, rel_x, rel_y);
                    }
                    changed = true;
                }
            }

            changed | apply_click_focus_and_tab(x, y, cols, rows, state, model)
        }
        _ => false,
    }
}

fn apply_click_focus_and_tab(
    col: u16,
    row: u16,
    cols: u16,
    rows: u16,
    state: &mut TuiState,
    model: &str,
) -> bool {
    let chat_width = cols.saturating_mul(65) / 100;
    let defer_to_release = col >= chat_width;

    if col >= chat_width && row < super::draw::RIGHT_PANEL_TAB_ROWS {
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
            return false;
        }
        let mut changed = false;
        if state.tab != new_tab {
            state.tab = new_tab;
            changed = true;
            match state.tab {
                RightTab::Workspace => refresh_workspace(state),
                RightTab::Tasks => refresh_tasks(state),
                RightTab::Schedule => refresh_schedule(state),
            }
        }
        if state.focus != new_focus {
            state.focus = new_focus;
            changed = true;
        }
        if changed {
            set_normal_status_line(state, model);
        }
        return changed;
    }

    let new_focus = if col < chat_width {
        let below_input = super::draw::LEFT_COLUMN_ROWS_BELOW_INPUT;
        let input_start_row = rows.saturating_sub(state.input_rows.saturating_add(below_input));
        if row >= input_start_row && row < rows.saturating_sub(below_input) {
            Focus::ChatInput
        } else {
            Focus::ChatView
        }
    } else if state.tab == RightTab::Workspace && row >= super::draw::RIGHT_PANEL_ROWS_ABOVE_CONTENT
    {
        Focus::Workspace
    } else {
        Focus::Right
    };

    if new_focus != state.focus {
        if defer_to_release {
            state.pending_focus = Some(new_focus);
            return false;
        }
        state.focus = new_focus;
        set_normal_status_line(state, model);
        return true;
    }
    false
}

fn apply_pending_focus_and_tab(state: &mut TuiState, model: &str) -> bool {
    let mut changed = false;

    if let Some(tab) = state.pending_tab.take()
        && state.tab != tab
    {
        state.tab = tab;
        changed = true;
        match state.tab {
            RightTab::Workspace => refresh_workspace(state),
            RightTab::Tasks => refresh_tasks(state),
            RightTab::Schedule => refresh_schedule(state),
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
    changed
}
