//! TUI 分区布局绘制与聊天区滚动估算。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use unicode_width::UnicodeWidthStr;

use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
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
    // 顶栏仅 CrabMate · 工作目录；模型/base_url 在底栏；三列 + 底栏
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
        " 会话 ",
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
    let chat_inner = chat_block_inner_area(panes.chat);
    let (text_rect, scrollbar_rect) = chat_inner_split_text_and_scrollbar(chat_inner);
    let tw = text_rect.width.max(1);
    let th = text_rect.height.max(1);
    let chat_scroll_y = clamped_chat_vertical_scroll(
        chat_body.as_str(),
        tw,
        th,
        streaming_nonempty,
        model.chat_scroll_y,
    );
    let rows = estimate_wrapped_line_rows(chat_body.as_str(), tw);
    let vis_lines = th as usize;

    frame.render_widget(chat_block, panes.chat);
    let center_body = Paragraph::new(chat_body)
        .wrap(Wrap { trim: false })
        .scroll((chat_scroll_y, 0));
    frame.render_widget(center_body, text_rect);

    if scrollbar_rect.width > 0 && rows > vis_lines {
        let bar_style = scrollbar_track_style(color, model.focus == TuiFocus::Chat);
        let mut sb_state = ScrollbarState::new(rows.saturating_sub(vis_lines).saturating_add(1))
            .position(usize::from(chat_scroll_y))
            .viewport_content_length(vis_lines);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .symbols(scrollbar::VERTICAL)
            .style(bar_style);
        frame.render_stateful_widget(scrollbar, scrollbar_rect, &mut sb_state);
    }

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
        " 工作区 ",
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

/// 与绘制一致的聊天面板 content 区（`Block` 边框 + 标题占用与 [`panel_block`] 一致）。
pub(super) fn chat_block_inner_area(chat_pane: Rect) -> Rect {
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(" 聊天 "))
        .inner(chat_pane)
}

/// 纵向滚动条可交互时的几何与 `max_scroll`（内容未溢出时返回 `None`）。
pub(super) struct ChatScrollbarHit {
    pub(super) rect: Rect,
    pub(super) max_scroll: u16,
}

pub(super) fn chat_scrollbar_hit(
    chat_pane: Rect,
    transcript: &str,
    scratch: &TuiLlmStreamScratch,
) -> Option<ChatScrollbarHit> {
    let chat_inner = chat_block_inner_area(chat_pane);
    let (text_rect, sb_rect) = chat_inner_split_text_and_scrollbar(chat_inner);
    if sb_rect.width == 0 {
        return None;
    }
    let chat_body = append_tui_streaming_tail(transcript, scratch);
    let tw = text_rect.width.max(1);
    let th = text_rect.height.max(1);
    let rows = estimate_wrapped_line_rows(chat_body.as_str(), tw);
    let vis_lines = th as usize;
    if rows <= vis_lines {
        return None;
    }
    let max_scroll = rows.saturating_sub(vis_lines).min(u16::MAX as usize) as u16;
    Some(ChatScrollbarHit {
        rect: sb_rect,
        max_scroll,
    })
}

/// 将指针所在行映射为 `Paragraph::scroll` 的 `y`（按轨道比例；行坐标可落在轨道外，仍 clamp）。
pub(super) fn scrollbar_row_to_scroll_y(row: u16, hit: &ChatScrollbarHit) -> u16 {
    if hit.max_scroll == 0 {
        return 0;
    }
    let h = hit.rect.height.max(1);
    let rel = row.saturating_sub(hit.rect.y).min(h.saturating_sub(1));
    let denom = u32::from(h.saturating_sub(1).max(1));
    let num = u32::from(rel) * u32::from(hit.max_scroll);
    (num / denom).min(u32::from(hit.max_scroll)) as u16
}

/// 聊天区内：左侧正文，右侧预留 1 列滚动条（宽度不足时仅占正文）。
pub(super) fn chat_inner_split_text_and_scrollbar(inner: Rect) -> (Rect, Rect) {
    if inner.width >= 2 && inner.height >= 1 {
        let text_w = inner.width.saturating_sub(1);
        (
            Rect::new(inner.x, inner.y, text_w, inner.height),
            Rect::new(inner.x.saturating_add(text_w), inner.y, 1, inner.height),
        )
    } else {
        (inner, Rect::new(0, 0, 0, 0))
    }
}

fn scrollbar_track_style(color: bool, chat_focused: bool) -> Style {
    if color {
        let fg = if chat_focused {
            Color::DarkGray
        } else {
            Color::Rgb(55, 58, 66)
        };
        Style::default().fg(fg)
    } else {
        Style::default()
    }
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
