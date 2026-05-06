//! TUI 分区布局绘制与聊天区滚动估算。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::runtime::tui::TuiLlmStreamScratchArc;
use crate::text_util::truncate_chars_with_ellipsis;

use super::approval;
use super::{TuiFocus, TuiModel};

pub(super) fn append_tui_streaming_tail(
    transcript: &str,
    scratch: &crate::runtime::tui::TuiLlmStreamScratch,
) -> String {
    let r = scratch.reasoning.trim();
    let c = scratch.content.trim();
    if r.is_empty() && c.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::from(transcript);
    out.push_str("\n────────────────────────────────\n[assistant · 生成中]\n\n");
    if !r.is_empty() {
        out.push_str("(推理) ");
        out.push_str(&truncate_chars_with_ellipsis(r, 8000));
        out.push_str("\n\n");
    }
    if !c.is_empty() {
        out.push_str(&truncate_chars_with_ellipsis(c, 12000));
        out.push('\n');
    }
    out
}

/// 粗算 `Paragraph` + `Wrap` 下的总行数（与 ratatui `WordWrapper` 不完全一致；用于 **限制 scroll_y**，避免 `area.height + scroll_y` 的 `u16` 溢出与 panic）。
pub(super) fn estimate_wrapped_line_rows(text: &str, inner_width: u16) -> usize {
    let w = inner_width.max(1) as usize;
    if text.is_empty() {
        return 1;
    }
    text.split('\n')
        .map(|line| {
            let lw = UnicodeWidthStr::width(line);
            lw.div_ceil(w).max(1)
        })
        .sum::<usize>()
        .max(1)
}

/// ratatui 0.29：`Paragraph::scroll` 的 `y` 不得大到使内部 `area.height + scroll_y` 溢出；也不得大于「总行数 − 视口行数」。
pub(super) fn clamped_chat_vertical_scroll(
    text: &str,
    inner_width: u16,
    inner_height: u16,
    stick_to_bottom: bool,
    manual_scroll_y: u16,
) -> u16 {
    let rows = estimate_wrapped_line_rows(text, inner_width);
    let vis = inner_height.max(1) as usize;
    let max_scroll = rows.saturating_sub(vis).min(u16::MAX as usize) as u16;
    if stick_to_bottom {
        max_scroll
    } else {
        manual_scroll_y.min(max_scroll)
    }
}

pub(super) fn render_full(
    frame: &mut Frame<'_>,
    model: &TuiModel,
    llm_scratch: &TuiLlmStreamScratchArc,
    color: bool,
) {
    let area = frame.area();
    // 对齐 Web `shell-ds`：顶栏 + 三列（侧栏宽≈ nav-rail）+ 底栏快捷键
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    render_top_bar(frame, vertical[0], model.header_line.as_str(), color);

    let panes = super::compute_tui_pane_layout(area);

    render_side_panel(
        frame,
        panes.nav_left,
        " 导航 · 会话 ",
        model.nav_summary.as_str(),
        color,
        model.focus == TuiFocus::NavLeft,
    );

    // 「撰写」块含四边边框 + 标题，高度若仅 1 行会导致内层为 0。
    let scratch_guard = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
    let streaming_nonempty =
        !scratch_guard.reasoning.trim().is_empty() || !scratch_guard.content.trim().is_empty();
    let chat_body = append_tui_streaming_tail(model.transcript.as_str(), &scratch_guard);
    drop(scratch_guard);
    let chat_block = panel_block(" 聊天 ", color, model.focus == TuiFocus::Chat);
    let chat_inner = chat_block.inner(panes.chat);
    let chat_scroll_y = clamped_chat_vertical_scroll(
        chat_body.as_str(),
        chat_inner.width.max(1),
        chat_inner.height.max(1),
        streaming_nonempty,
        model.chat_scroll_y,
    );
    let center_body = Paragraph::new(chat_body)
        .wrap(Wrap { trim: false })
        .scroll((chat_scroll_y, 0))
        .block(chat_block);
    frame.render_widget(center_body, panes.chat);

    let composer_block = panel_block(" 撰写 ", color, model.focus == TuiFocus::Composer);
    let composer_inner = composer_block.inner(panes.composer);
    let (composer_text, cursor_rel) =
        super::composer_visible_and_cursor_rel(composer_inner, model.input.as_str());
    let composer_style = if color && model.focus == TuiFocus::Composer {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input_par = Paragraph::new(composer_text)
        .style(composer_style)
        .block(composer_block);
    frame.render_widget(input_par, panes.composer);
    if model.approval_modal.is_none()
        && model.clarification_modal.is_none()
        && model.workspace_modal.is_none()
        && model.focus == TuiFocus::Composer
        && let Some((cx, cy)) = cursor_rel
    {
        frame.set_cursor_position(Position::new(
            composer_inner.x.saturating_add(cx),
            composer_inner.y.saturating_add(cy),
        ));
    }

    render_side_panel(
        frame,
        panes.side_right,
        " 侧栏 · 任务 ",
        model.right_summary.as_str(),
        color,
        model.focus == TuiFocus::SideRight,
    );

    let status_style = status_line_style(color);
    let status_block = Block::default().style(status_style);
    let status_w = vertical[2].width.saturating_sub(2).max(8) as usize;
    let status_text = truncate_chars_with_ellipsis(model.status.as_str(), status_w);
    let status_line = if color {
        Line::from(Span::styled(
            status_text.as_str(),
            Style::default().fg(Color::White),
        ))
    } else {
        Line::from(status_text)
    };
    let status = Paragraph::new(status_line).block(status_block);
    frame.render_widget(status, vertical[2]);

    if let Some(ref modal) = model.approval_modal {
        approval::render_approval_modal(frame, area, modal, color);
    }
    if let Some(ref cq) = model.clarification_modal {
        super::clarify_modal::render_clarification_modal(frame, area, cq, color);
    }
    if let Some(ref ws) = model.workspace_modal {
        super::workspace_modal::render_workspace_modal(frame, area, ws, color);
    }
}

fn render_top_bar(frame: &mut Frame<'_>, area: Rect, header: &str, color: bool) {
    let max_w = area.width.saturating_sub(2).max(4) as usize;
    let text = truncate_chars_with_ellipsis(header, max_w);
    let fg = if color {
        Color::Rgb(200, 204, 212)
    } else {
        Color::Reset
    };
    let bg = if color {
        Color::Rgb(40, 44, 52)
    } else {
        Color::Reset
    };
    let line = Line::from(Span::styled(text, Style::default().fg(fg).bg(bg)));
    let block_style = if color {
        Style::default().bg(bg)
    } else {
        Style::default()
    };
    let p = Paragraph::new(line).block(Block::default().style(block_style));
    frame.render_widget(p, area);
}

fn panel_block(title: &str, color: bool, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(title))
        .title_style(title_style(color, focused))
        .border_style(panel_border_style(color, focused))
}

fn panel_border_style(color: bool, focused: bool) -> Style {
    if color {
        if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn render_side_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    body: &str,
    color: bool,
    focused: bool,
) {
    let paragraph = Paragraph::new(body)
        .wrap(Wrap { trim: true })
        .block(panel_block(title, color, focused));
    frame.render_widget(paragraph, area);
}

fn title_style(color: bool, focused: bool) -> Style {
    if color {
        if focused {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::Cyan)
        }
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn status_line_style(color: bool) -> Style {
    if color {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default()
    }
}
