//! 澄清问卷全屏 Modal：与 Web `merge_user_text_with_clarification_answers` 语义对齐。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::clarification_questionnaire::{
    ClarifyAnswersNormalized, normalize_clarify_questionnaire_answers_raw,
};
use crate::sse::ClarificationQuestionnaireBody;

use super::{TuiModel, UiEvent};

pub(super) struct TuiClarificationModalState {
    pub(super) body: ClarificationQuestionnaireBody,
    pub(super) answers: Vec<String>,
    pub(super) focused_q: usize,
    pub(super) error_line: Option<String>,
}

impl TuiClarificationModalState {
    pub(super) fn new(body: ClarificationQuestionnaireBody) -> Self {
        let n = body.questions.len();
        Self {
            body,
            answers: vec![String::new(); n],
            focused_q: 0,
            error_line: None,
        }
    }
}

pub(super) enum ClarificationModalKeyOutcome {
    NotApplicable,
    Consumed,
}

pub(super) fn enqueue_clarification_from_hook(
    inbox: &Arc<Mutex<VecDeque<ClarificationQuestionnaireBody>>>,
    model: &Arc<Mutex<TuiModel>>,
    body: ClarificationQuestionnaireBody,
) {
    if let Ok(mut q) = inbox.lock() {
        q.push_back(body);
    }
    drain_clarification_inbox(inbox, model);
}

fn drain_clarification_inbox(
    inbox: &Arc<Mutex<VecDeque<ClarificationQuestionnaireBody>>>,
    model: &Arc<Mutex<TuiModel>>,
) {
    let mut drained: Vec<ClarificationQuestionnaireBody> = Vec::new();
    if let Ok(mut q) = inbox.lock() {
        drained.extend(q.drain(..));
    }
    if drained.is_empty() {
        return;
    }
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    for b in drained {
        if g.clarification_modal.is_none() && g.clarification_backlog.is_empty() {
            g.clarification_modal = Some(TuiClarificationModalState::new(b));
        } else {
            g.clarification_backlog.push_back(b);
        }
    }
}

pub(super) fn poll_clarification_inbox(
    inbox: &Arc<Mutex<VecDeque<ClarificationQuestionnaireBody>>>,
    model: &Arc<Mutex<TuiModel>>,
) {
    drain_clarification_inbox(inbox, model);
}

pub(super) fn dismiss_clarification_modal(model: &mut TuiModel) {
    model.clarification_modal.take();
    if let Some(next) = model.clarification_backlog.pop_front() {
        model.clarification_modal = Some(TuiClarificationModalState::new(next));
    }
}

fn submit_clarification_modal(
    model: &mut TuiModel,
    clarify_merge: &Arc<Mutex<Option<ClarifyAnswersNormalized>>>,
    ev_tx: &UnboundedSender<UiEvent>,
) {
    let Some(state) = model.clarification_modal.as_mut() else {
        return;
    };
    let n = state.body.questions.len();
    if n == 0 {
        dismiss_clarification_modal(model);
        return;
    }
    for i in 0..n {
        let req = state.body.questions[i].required == Some(true);
        let v = state.answers.get(i).map(|s| s.trim()).unwrap_or("");
        if req && v.is_empty() {
            state.error_line = Some(format!("题目 `{}` 为必填", state.body.questions[i].id));
            return;
        }
    }
    let mut map = serde_json::Map::new();
    for (i, q) in state.body.questions.iter().enumerate() {
        let v = state.answers.get(i).map(|s| s.trim()).unwrap_or("");
        map.insert(q.id.clone(), Value::String(v.to_string()));
    }
    let qid = state.body.questionnaire_id.clone();
    let normalized = match normalize_clarify_questionnaire_answers_raw(qid, Value::Object(map)) {
        Ok(Some(norm)) => norm,
        Ok(None) => {
            state.error_line = Some("问卷 id 为空".to_string());
            return;
        }
        Err(e) => {
            state.error_line = Some(e);
            return;
        }
    };

    model.clarification_modal.take();
    if let Ok(mut slot) = clarify_merge.lock() {
        *slot = Some(normalized);
    }
    let composer_snapshot = std::mem::take(&mut model.input);
    let _ = ev_tx.send(UiEvent::Submit(composer_snapshot));
    if let Some(next) = model.clarification_backlog.pop_front() {
        model.clarification_modal = Some(TuiClarificationModalState::new(next));
    }
}

pub(super) fn handle_clarification_modal_keys(
    model: &Arc<Mutex<TuiModel>>,
    clarify_merge: &Arc<Mutex<Option<ClarifyAnswersNormalized>>>,
    ev_tx: &UnboundedSender<UiEvent>,
    key: &event::KeyEvent,
) -> ClarificationModalKeyOutcome {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    if g.clarification_modal.is_none() {
        return ClarificationModalKeyOutcome::NotApplicable;
    }

    match key.code {
        KeyCode::Esc => {
            dismiss_clarification_modal(&mut g);
            ClarificationModalKeyOutcome::Consumed
        }
        KeyCode::Enter => {
            submit_clarification_modal(&mut g, clarify_merge, ev_tx);
            ClarificationModalKeyOutcome::Consumed
        }
        KeyCode::Tab => {
            if let Some(ref mut st) = g.clarification_modal {
                let n = st.body.questions.len().max(1);
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    st.focused_q = st.focused_q.saturating_sub(1);
                } else {
                    st.focused_q = (st.focused_q + 1).min(n.saturating_sub(1));
                }
                st.error_line = None;
            }
            ClarificationModalKeyOutcome::Consumed
        }
        KeyCode::Backspace => {
            if let Some(ref mut st) = g.clarification_modal {
                let i = st.focused_q.min(st.answers.len().saturating_sub(1));
                if let Some(a) = st.answers.get_mut(i) {
                    a.pop();
                }
                st.error_line = None;
            }
            ClarificationModalKeyOutcome::Consumed
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(ref mut st) = g.clarification_modal {
                let i = st.focused_q.min(st.answers.len().saturating_sub(1));
                if let Some(a) = st.answers.get_mut(i) {
                    a.push(ch);
                }
                st.error_line = None;
            }
            ClarificationModalKeyOutcome::Consumed
        }
        _ => ClarificationModalKeyOutcome::Consumed,
    }
}

pub(super) fn render_clarification_modal(
    frame: &mut Frame<'_>,
    area: Rect,
    modal: &TuiClarificationModalState,
    color: bool,
) {
    let block_w = area.width.clamp(20, 76);
    let block_h = area.height.max(8);
    let x = area
        .x
        .saturating_add(area.width.saturating_sub(block_w) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(block_h) / 2);
    let rect = Rect::new(x, y, block_w, block_h);
    frame.render_widget(Clear, rect);

    let inner = Block::default().borders(Borders::ALL).title(Span::styled(
        " 澄清问卷 ",
        if color {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        },
    ));
    let inner_area = inner.inner(rect);
    frame.render_widget(inner, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(2),
            Constraint::Length(2),
        ])
        .split(inner_area);

    let intro = Paragraph::new(modal.body.intro.as_str())
        .wrap(Wrap { trim: true })
        .alignment(Alignment::Left);
    frame.render_widget(intro, chunks[0]);

    let mut lines: Vec<Line> = Vec::new();
    for (i, q) in modal.body.questions.iter().enumerate() {
        let focus = i == modal.focused_q;
        let prefix = if focus { "› " } else { "  " };
        let req = if q.required == Some(true) { " *" } else { "" };
        let hint = q.hint.as_deref().unwrap_or("");
        let hint_s = if hint.is_empty() {
            String::new()
        } else {
            format!(" ({hint})")
        };
        let label_style = if focus && color {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(format!("{}{}", q.label, req), label_style),
            Span::styled(hint_s, Style::default().fg(Color::DarkGray)),
        ]));
        let ans = modal.answers.get(i).cloned().unwrap_or_default();
        lines.push(Line::from(format!("    [{id}] {ans}", id = q.id)));
        lines.push(Line::from(""));
    }
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, chunks[1]);

    let footer = if let Some(ref e) = modal.error_line {
        Line::from(Span::styled(
            format!("错误：{e}"),
            Style::default().fg(Color::Red),
        ))
    } else {
        Line::from("Tab 切换题目 · 作答 · Enter 提交（附撰写区正文） · Esc 跳过")
    };
    let fp = Paragraph::new(footer).alignment(Alignment::Center);
    frame.render_widget(fp, chunks[2]);
}
