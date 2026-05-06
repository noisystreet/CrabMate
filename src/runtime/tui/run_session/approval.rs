//! TUI 敏感工具审批 Modal（队列与键盘）。

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tokio::sync::mpsc::UnboundedSender;

use crate::text_util::truncate_chars_with_ellipsis;
use crate::tool_approval::TuiApprovalRequest;
use crate::types::CommandApprovalDecision;

use super::{TuiModel, UiEvent};

/// 与 [`crate::tool_approval::cli_terminal`] 菜单顺序一致。
pub(super) const APPROVAL_MODAL_LABELS: [&str; 3] = [
    "拒绝（n / Esc）",
    "本次允许（y）",
    "永久允许该键，本会话（a）",
];

pub(super) struct TuiApprovalModalState {
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) respond_tx: std::sync::mpsc::Sender<CommandApprovalDecision>,
    /// 0..=2，与 [`APPROVAL_MODAL_LABELS`] 对齐。
    pub(super) selected: usize,
}

impl TuiApprovalModalState {
    pub(super) fn new(req: TuiApprovalRequest) -> Self {
        Self {
            title: req.title,
            detail: req.detail,
            respond_tx: req.respond_tx,
            selected: 0,
        }
    }

    pub(super) fn decision_for_index(idx: usize) -> CommandApprovalDecision {
        match idx {
            0 => CommandApprovalDecision::Deny,
            1 => CommandApprovalDecision::AllowOnce,
            2 => CommandApprovalDecision::AllowAlways,
            _ => CommandApprovalDecision::Deny,
        }
    }
}

pub(super) enum ApprovalModalKeyOutcome {
    NotApplicable,
    Consumed,
    QuitApp,
}

pub(super) fn finish_approval_modal(g: &mut TuiModel, decision: CommandApprovalDecision) {
    if let Some(state) = g.approval_modal.take() {
        let _ = state.respond_tx.send(decision);
        if let Some(next) = g.approval_backlog.pop_front() {
            g.approval_modal = Some(TuiApprovalModalState::new(next));
        }
    }
}

pub(super) fn deny_all_pending_approvals(g: &mut TuiModel) {
    while g.approval_modal.is_some() {
        finish_approval_modal(g, CommandApprovalDecision::Deny);
    }
    for req in g.approval_backlog.drain(..) {
        let _ = req.respond_tx.send(CommandApprovalDecision::Deny);
    }
}

pub(super) fn handle_approval_modal_keys(
    model: &Arc<Mutex<TuiModel>>,
    ev_tx: &UnboundedSender<UiEvent>,
    key: &event::KeyEvent,
) -> ApprovalModalKeyOutcome {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    if g.approval_modal.is_none() {
        return ApprovalModalKeyOutcome::NotApplicable;
    }

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        deny_all_pending_approvals(&mut g);
        drop(g);
        let _ = ev_tx.send(UiEvent::Quit);
        return ApprovalModalKeyOutcome::QuitApp;
    }

    let Some(modal) = g.approval_modal.as_mut() else {
        return ApprovalModalKeyOutcome::Consumed;
    };

    match key.code {
        KeyCode::Esc => {
            finish_approval_modal(&mut g, CommandApprovalDecision::Deny);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Enter => {
            let d = TuiApprovalModalState::decision_for_index(modal.selected);
            finish_approval_modal(&mut g, d);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
            modal.selected = modal.selected.saturating_sub(1);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
            modal.selected = (modal.selected + 1).min(2);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('1') => {
            modal.selected = 0;
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('2') => {
            modal.selected = 1;
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('3') => {
            modal.selected = 2;
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('y') | KeyCode::Char('Y')
            if !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            finish_approval_modal(&mut g, CommandApprovalDecision::AllowOnce);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('n') | KeyCode::Char('N')
            if !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            finish_approval_modal(&mut g, CommandApprovalDecision::Deny);
            ApprovalModalKeyOutcome::Consumed
        }
        KeyCode::Char('a') | KeyCode::Char('A')
            if !key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            finish_approval_modal(&mut g, CommandApprovalDecision::AllowAlways);
            ApprovalModalKeyOutcome::Consumed
        }
        _ => ApprovalModalKeyOutcome::Consumed,
    }
}

pub(super) fn enqueue_tui_approval_requests(
    model: &Arc<Mutex<TuiModel>>,
    approval_rx: &Receiver<TuiApprovalRequest>,
) {
    while let Ok(req) = approval_rx.try_recv() {
        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
        if g.approval_modal.is_some() {
            g.approval_backlog.push_back(req);
        } else {
            g.approval_modal = Some(TuiApprovalModalState::new(req));
        }
    }
}

fn approval_modal_rect(area: Rect) -> Rect {
    let w = (area.width.saturating_mul(4) / 5)
        .max(28)
        .min(area.width.saturating_sub(2));
    let h = (area.height.saturating_mul(3) / 5)
        .max(12)
        .min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

pub(super) fn render_approval_modal(
    frame: &mut Frame<'_>,
    full_area: Rect,
    modal: &TuiApprovalModalState,
    color: bool,
) {
    let popup = approval_modal_rect(full_area);
    frame.render_widget(Clear, popup);
    let border_style = if color {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(" 敏感操作审批 "))
        .border_style(border_style);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 6 || inner.width < 8 {
        return;
    }

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Min(2),
            ratatui::layout::Constraint::Length(7),
        ])
        .split(inner);

    let title_style = if color {
        Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let tw = chunks[0].width.max(1) as usize;
    let title_txt = truncate_chars_with_ellipsis(modal.title.trim(), tw.max(12));
    frame.render_widget(Paragraph::new(title_txt).style(title_style), chunks[0]);

    frame.render_widget(
        Paragraph::new(modal.detail.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default()),
        chunks[1],
    );

    let sel_style = if color {
        Style::default().bg(Color::Cyan).fg(Color::Black)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, label) in APPROVAL_MODAL_LABELS.iter().enumerate() {
        let prefix = if i == modal.selected { "▸ " } else { "  " };
        let sty = if i == modal.selected {
            sel_style
        } else if color {
            Style::default().fg(Color::Gray)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{label}"), sty)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓ j/k · Enter · y/n/a · 1/2/3 · Esc",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(Text::from(lines)), chunks[2]);
}
