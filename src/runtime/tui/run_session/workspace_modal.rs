//! 工作区切换 Modal：目录浏览 + 手动路径，校验与 REPL **`/workspace`** / Web **`POST /workspace`** 同源（[`crate::tools::resolve_repl_workspace_switch_path`]）。

use std::path::PathBuf;

use crossterm::event::{self, KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tokio::sync::mpsc::UnboundedSender;

use crate::text_util::truncate_chars_with_ellipsis;

use super::{TuiModel, UiEvent};

const ENTRY_CAP: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceModalFocus {
    DirList,
    PathInput,
}

pub(super) struct TuiWorkspaceModalState {
    /// 当前浏览目录（绝对路径优先）。
    pub(super) browse_dir: PathBuf,
    /// 列表项：`..`（若有父目录）+ 子目录名。
    pub(super) entries: Vec<String>,
    pub(super) cursor: usize,
    pub(super) list_scroll: usize,
    pub(super) path_input: String,
    pub(super) focus_field: WorkspaceModalFocus,
    pub(super) error_line: Option<String>,
}

impl TuiWorkspaceModalState {
    pub(super) fn open(initial_workspace: PathBuf) -> Self {
        let browse_dir = initial_workspace
            .canonicalize()
            .unwrap_or(initial_workspace);
        let mut s = Self {
            browse_dir,
            entries: Vec::new(),
            cursor: 0,
            list_scroll: 0,
            path_input: String::new(),
            focus_field: WorkspaceModalFocus::DirList,
            error_line: None,
        };
        s.refresh_entries();
        s
    }

    fn refresh_entries(&mut self) {
        self.error_line = None;
        let mut rows: Vec<String> = Vec::new();
        if self.browse_dir.parent().is_some() {
            rows.push("..".to_string());
        }
        match std::fs::read_dir(&self.browse_dir) {
            Ok(rd) => {
                let mut dirs: Vec<String> = Vec::new();
                for ent in rd.flatten() {
                    let Ok(ft) = ent.file_type() else {
                        continue;
                    };
                    if ft.is_dir() {
                        dirs.push(ent.file_name().to_string_lossy().into_owned());
                    }
                }
                dirs.sort();
                dirs.truncate(ENTRY_CAP.saturating_sub(rows.len()));
                rows.extend(dirs);
            }
            Err(e) => {
                self.error_line = Some(format!("无法列出目录：{e}"));
            }
        }
        self.entries = rows;
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        self.list_scroll = self.list_scroll.min(self.entries.len().saturating_sub(1));
        self.ensure_cursor_visible(8);
    }

    fn ensure_cursor_visible(&mut self, viewport_lines: usize) {
        if self.entries.is_empty() {
            self.list_scroll = 0;
            return;
        }
        let max_scroll = self.entries.len().saturating_sub(viewport_lines.max(1));
        if self.cursor < self.list_scroll {
            self.list_scroll = self.cursor;
        } else if self.cursor >= self.list_scroll.saturating_add(viewport_lines) {
            self.list_scroll = self.cursor.saturating_sub(viewport_lines.saturating_sub(1));
        }
        self.list_scroll = self.list_scroll.min(max_scroll);
    }

    fn descend_or_ascend(&mut self, entry: &str) {
        self.error_line = None;
        if entry == ".." {
            if let Some(p) = self.browse_dir.parent() {
                self.browse_dir = p.to_path_buf();
                let _ = self.browse_dir.canonicalize().map(|c| {
                    self.browse_dir = c;
                });
                self.cursor = 0;
                self.list_scroll = 0;
                self.refresh_entries();
            }
            return;
        }
        let next = self.browse_dir.join(entry);
        if next.is_dir() {
            match next.canonicalize() {
                Ok(c) => {
                    self.browse_dir = c;
                    self.cursor = 0;
                    self.list_scroll = 0;
                    self.refresh_entries();
                }
                Err(e) => {
                    self.error_line = Some(format!("无法进入目录：{e}"));
                }
            }
        } else {
            self.error_line = Some("不是目录".to_string());
        }
    }
}

pub(super) enum WorkspaceModalKeyOutcome {
    NotApplicable,
    Consumed,
}

pub(super) fn handle_workspace_modal_keys(
    model: &std::sync::Arc<std::sync::Mutex<TuiModel>>,
    ev_tx: &UnboundedSender<UiEvent>,
    key: &event::KeyEvent,
) -> WorkspaceModalKeyOutcome {
    let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ref mut st) = g.workspace_modal else {
        return WorkspaceModalKeyOutcome::NotApplicable;
    };

    match key.code {
        KeyCode::Esc => {
            g.workspace_modal = None;
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Tab => {
            st.focus_field = match st.focus_field {
                WorkspaceModalFocus::DirList => WorkspaceModalFocus::PathInput,
                WorkspaceModalFocus::PathInput => WorkspaceModalFocus::DirList,
            };
            st.error_line = None;
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Char('y') | KeyCode::Char('Y')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && st.focus_field == WorkspaceModalFocus::DirList =>
        {
            let raw = st.browse_dir.to_string_lossy().into_owned();
            g.workspace_modal = None;
            drop(g);
            let _ = ev_tx.send(UiEvent::WorkspaceSwitch(raw));
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Char('r') | KeyCode::Char('R')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && st.focus_field == WorkspaceModalFocus::DirList =>
        {
            st.refresh_entries();
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Up => {
            if st.focus_field == WorkspaceModalFocus::DirList && !st.entries.is_empty() {
                st.cursor = st.cursor.saturating_sub(1);
                st.ensure_cursor_visible(8);
                st.error_line = None;
            }
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Down => {
            if st.focus_field == WorkspaceModalFocus::DirList && !st.entries.is_empty() {
                let n = st.entries.len().saturating_sub(1);
                st.cursor = (st.cursor + 1).min(n);
                st.ensure_cursor_visible(8);
                st.error_line = None;
            }
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Enter => {
            match st.focus_field {
                WorkspaceModalFocus::DirList => {
                    if let Some(name) = st.entries.get(st.cursor).cloned() {
                        st.descend_or_ascend(name.as_str());
                    }
                }
                WorkspaceModalFocus::PathInput => {
                    let raw = st.path_input.trim().to_string();
                    if raw.is_empty() {
                        st.error_line = Some("路径不能为空".to_string());
                        return WorkspaceModalKeyOutcome::Consumed;
                    }
                    g.workspace_modal = None;
                    drop(g);
                    let _ = ev_tx.send(UiEvent::WorkspaceSwitch(raw));
                }
            }
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Backspace => {
            if st.focus_field == WorkspaceModalFocus::PathInput {
                st.path_input.pop();
                st.error_line = None;
            }
            WorkspaceModalKeyOutcome::Consumed
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if st.focus_field == WorkspaceModalFocus::PathInput {
                st.path_input.push(ch);
                st.error_line = None;
            }
            WorkspaceModalKeyOutcome::Consumed
        }
        _ => WorkspaceModalKeyOutcome::Consumed,
    }
}

pub(super) fn render_workspace_modal(
    frame: &mut Frame<'_>,
    area: Rect,
    modal: &TuiWorkspaceModalState,
    color: bool,
) {
    let block_w = area.width.clamp(22, 78);
    let block_h = area.height.max(10).min(area.height);
    let x = area
        .x
        .saturating_add(area.width.saturating_sub(block_w) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(block_h) / 2);
    let rect = Rect::new(x, y, block_w, block_h);
    frame.render_widget(Clear, rect);

    let inner = Block::default().borders(Borders::ALL).title(Span::styled(
        " 工作区 ",
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
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner_area);

    let cur_disp = modal.browse_dir.display().to_string();
    let intro = Paragraph::new(Line::from(vec![
        Span::raw("当前："),
        Span::styled(
            truncate_for_modal(&cur_disp, block_w.saturating_sub(6) as usize),
            if color {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            },
        ),
    ]))
    .alignment(Alignment::Left);
    frame.render_widget(intro, chunks[0]);

    let list_h = chunks[1].height.max(1) as usize;
    let viewport = list_h.saturating_sub(1).max(1);
    let mut lines: Vec<Line> = Vec::new();
    let list_style = modal.focus_field == WorkspaceModalFocus::DirList;
    let title = if list_style {
        Line::from(Span::styled(
            "目录（↑↓ Enter 进入 · y 选用 · r 刷新）",
            Style::default().add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from("目录（↑↓ Enter 进入 · y 选用 · r 刷新）")
    };
    lines.push(title);
    let start = modal.list_scroll;
    let end = (start + viewport).min(modal.entries.len());
    for i in start..end {
        let focus = i == modal.cursor;
        let name = modal.entries[i].as_str();
        let prefix = if focus { "› " } else { "  " };
        let sty = if focus && color {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(name.to_string(), sty),
        ]));
    }
    if modal.entries.is_empty() && modal.error_line.is_none() {
        lines.push(Line::from("(空目录)"));
    }
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, chunks[1]);

    let inp_focus = modal.focus_field == WorkspaceModalFocus::PathInput;
    let inp_label = Paragraph::new(if inp_focus {
        Line::from(Span::styled(
            "手动路径（Tab 切到此 · Enter 应用）：",
            Style::default().add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from("手动路径（Tab 切到此 · Enter 应用）：")
    });
    frame.render_widget(inp_label, chunks[2]);

    let path_line = Paragraph::new(modal.path_input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(if inp_focus && color {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            }),
    );
    frame.render_widget(path_line, chunks[3]);

    let footer = if let Some(ref e) = modal.error_line {
        Paragraph::new(Line::from(Span::styled(
            format!("错误：{e}"),
            Style::default().fg(Color::Red),
        )))
    } else {
        Paragraph::new(Line::from(
            "Tab 列表/手动 · Esc 关闭 · 策略同 Web POST /workspace、REPL /workspace",
        ))
    };
    frame.render_widget(footer, chunks[4]);
}

fn truncate_for_modal(s: &str, max_chars: usize) -> String {
    truncate_chars_with_ellipsis(s, max_chars.max(4))
}
