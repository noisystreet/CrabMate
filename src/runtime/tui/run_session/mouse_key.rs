//! TUI 鼠标与键盘分派（焦点、滚动、Modal、提交）。

use std::sync::{Arc, Mutex};

use crossterm::event::{self, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::size as terminal_size;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::runtime::tui::TuiLlmStreamScratchArc;

use super::approval;
use super::model::{
    TuiFocus, TuiModel, UiEvent, compute_tui_pane_layout, focus_at_point, rect_contains,
};
use super::render;
use super::workspace_modal;

pub(crate) enum TuiPollKeyFlow {
    BreakLoop,
    ContinueOuter,
}

fn open_workspace_modal(model: &Arc<Mutex<TuiModel>>) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let initial = g.workspace_path_buf.clone();
    g.workspace_modal = Some(workspace_modal::TuiWorkspaceModalState::open(initial));
}

fn tui_any_modal_open(model: &Arc<Mutex<TuiModel>>) -> bool {
    let g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.approval_modal.is_some() || g.clarification_modal.is_some() || g.workspace_modal.is_some()
}

fn tui_dispatch_mouse_chat_scroll(model: &Arc<Mutex<TuiModel>>, kind: event::MouseEventKind) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.chat_scrollbar_dragging = false;
    g.focus = TuiFocus::Chat;
    match kind {
        event::MouseEventKind::ScrollUp => {
            render::note_chat_user_scroll_up(&mut g);
            g.chat_scroll_y = g.chat_scroll_y.saturating_sub(3);
        }
        event::MouseEventKind::ScrollDown => {
            render::note_chat_user_scroll_down(&mut g);
            g.chat_scroll_y = g.chat_scroll_y.saturating_add(3);
        }
        _ => {}
    }
}

fn tui_dispatch_mouse_scrollbar_drag(
    model: &Arc<Mutex<TuiModel>>,
    llm_scratch: &TuiLlmStreamScratchArc,
    layout: Rect,
    row: u16,
) {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = render::chat_scrollbar_hit(
        layout,
        g.transcript.as_str(),
        &g.turn_projection,
        g.control_plane_tail.as_str(),
        &scratch,
    ) {
        g.focus = TuiFocus::Chat;
        let y = render::scrollbar_row_to_scroll_y(row, &hit);
        render::apply_chat_scrollbar_follow_intent(&mut g, y, hit.max_scroll);
    }
}

fn tui_try_consume_mouse_scrollbar_down(
    model: &Arc<Mutex<TuiModel>>,
    llm_scratch: &TuiLlmStreamScratchArc,
    layout: Rect,
    column: u16,
    row: u16,
) -> bool {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = render::chat_scrollbar_hit(
        layout,
        g.transcript.as_str(),
        &g.turn_projection,
        g.control_plane_tail.as_str(),
        &scratch,
    ) && rect_contains(hit.rect, column, row)
    {
        g.chat_scrollbar_dragging = true;
        g.focus = TuiFocus::Chat;
        let y = render::scrollbar_row_to_scroll_y(row, &hit);
        render::apply_chat_scrollbar_follow_intent(&mut g, y, hit.max_scroll);
        true
    } else {
        false
    }
}

pub(crate) fn tui_dispatch_mouse(
    model: &Arc<Mutex<TuiModel>>,
    mouse: event::MouseEvent,
    llm_scratch: &TuiLlmStreamScratchArc,
) {
    if tui_any_modal_open(model) {
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
            tui_dispatch_mouse_chat_scroll(model, mouse.kind);
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
            if dragging {
                tui_dispatch_mouse_scrollbar_drag(model, llm_scratch, layout.chat, mouse.row);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if tui_try_consume_mouse_scrollbar_down(
                model,
                llm_scratch,
                layout.chat,
                mouse.column,
                mouse.row,
            ) {
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

fn tui_quit_app(model: &Arc<Mutex<TuiModel>>, ev_tx: &UnboundedSender<UiEvent>) -> TuiPollKeyFlow {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    approval::deny_all_pending_approvals(&mut g);
    let _ = ev_tx.send(UiEvent::Quit);
    TuiPollKeyFlow::BreakLoop
}

fn tui_handle_q_key(
    model: &Arc<Mutex<TuiModel>>,
    ev_tx: &UnboundedSender<UiEvent>,
    ch: char,
) -> TuiPollKeyFlow {
    let input_empty = {
        let g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.input.is_empty()
    };
    if input_empty {
        return tui_quit_app(model, ev_tx);
    }
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.input.push(ch);
    TuiPollKeyFlow::ContinueOuter
}

fn tui_handle_tab_key(model: &Arc<Mutex<TuiModel>>, key: &event::KeyEvent) -> TuiPollKeyFlow {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    g.focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
        g.focus.cycle_prev()
    } else {
        g.focus.cycle_next()
    };
    TuiPollKeyFlow::ContinueOuter
}

fn tui_handle_chat_paging_key(model: &Arc<Mutex<TuiModel>>, code: KeyCode) -> TuiPollKeyFlow {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    if g.focus != TuiFocus::Chat {
        return TuiPollKeyFlow::ContinueOuter;
    }
    match code {
        KeyCode::PageUp => {
            render::note_chat_user_scroll_up(&mut g);
            g.chat_scroll_y = g.chat_scroll_y.saturating_sub(8);
        }
        KeyCode::PageDown => {
            render::note_chat_user_scroll_down(&mut g);
            g.chat_scroll_y = g.chat_scroll_y.saturating_add(8);
        }
        KeyCode::Home => {
            render::note_chat_user_scroll_up(&mut g);
            g.chat_scroll_y = 0;
        }
        KeyCode::End => {
            g.chat_follow_bottom = true;
            g.chat_snap_bottom_next_draw = true;
        }
        _ => {}
    }
    TuiPollKeyFlow::ContinueOuter
}

fn tui_handle_enter_key(
    model: &Arc<Mutex<TuiModel>>,
    ev_tx: &UnboundedSender<UiEvent>,
) -> TuiPollKeyFlow {
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
        g.chat_follow_bottom = true;
        g.chat_snap_bottom_next_draw = true;
        std::mem::take(&mut g.input)
    };
    let _ = ev_tx.send(UiEvent::Submit(line));
    TuiPollKeyFlow::ContinueOuter
}

pub(crate) fn tui_dispatch_key_press(
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
            tui_quit_app(model, ev_tx)
        }
        KeyCode::Char(ch @ ('q' | 'Q')) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            tui_handle_q_key(model, ev_tx, ch)
        }
        KeyCode::BackTab => {
            let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
            g.focus = g.focus.cycle_prev();
            TuiPollKeyFlow::ContinueOuter
        }
        KeyCode::Tab => tui_handle_tab_key(model, key),
        KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
            tui_handle_chat_paging_key(model, key.code)
        }
        KeyCode::Enter => tui_handle_enter_key(model, ev_tx),
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
