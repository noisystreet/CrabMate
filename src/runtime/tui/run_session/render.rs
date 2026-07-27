//! TUI 分区布局绘制与聊天区滚动估算。

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use unicode_width::UnicodeWidthStr;

use crate::runtime::message_display::{
    assistant_markdown_source_for_display, assistant_raw_markdown_body_from_parts,
};
use crate::runtime::tui::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
use crate::text_util::truncate_chars_with_ellipsis;

use super::approval;
use super::turn_project::TuiTurnProjection;
use super::{TuiFocus, TuiModel};

/// 对齐 Web `STICK_NEAR_BOTTOM_GAP_PX`：距底 ≤ 此行数视为近底，可 re-pin。
pub(super) const STICK_NEAR_BOTTOM_ROWS: u16 = 2;
/// 对齐 Web `STICK_UNPIN_GAP_PX`：拖滚动条离底超过此行数则 unpin。
pub(super) const STICK_UNPIN_GAP_ROWS: u16 = 4;

/// 流式尾挂：形状与 [`super::transcript::messages_to_transcript`] 的 assistant 块一致
///（`[assistant]\n{body}\n\n`），避免收束后文字跳位；运行态仍只在底栏右侧。
pub(super) fn append_tui_streaming_tail(
    transcript: &str,
    scratch: &crate::runtime::tui::TuiLlmStreamScratch,
    projection: &TuiTurnProjection,
) -> String {
    let r = scratch.reasoning.trim();
    let c = scratch.content.trim();
    let hide_content = projection.should_hide_streaming_content(c);
    let body = streaming_assistant_body_matching_transcript(r, c, hide_content);
    if body.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::from(transcript);
    // transcript 通常已以 `\n\n` 结尾；勿再多插空行，否则相对终态会下移。
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("[assistant]\n");
    out.push_str(body.as_str());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out
}

/// 与终态 `assistant_markdown_source_for_message` 同源组装，截断上限对齐 transcript。
fn streaming_assistant_body_matching_transcript(
    reasoning: &str,
    content: &str,
    hide_content: bool,
) -> String {
    let c = if hide_content { "" } else { content };
    let raw = assistant_raw_markdown_body_from_parts(reasoning, c);
    let t = assistant_markdown_source_for_display(&raw);
    let trimmed = t.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        truncate_chars_with_ellipsis(trimmed, 12_000)
    }
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

/// 聊天区纵向滚动模式（[`clamped_chat_vertical_scroll`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChatVerticalStickMode {
    /// 用户拖动滚轮/滚动条后的手动位置（仍 clamp 在合法范围内）。
    Manual,
    /// 流式生成中贴底（按估算折行的严格上限，不加 slack）。
    StreamStickBottom,
    /// 回合结束 / End / 发送后首帧贴底（同上，避免 slack 造成底部大块空白）。
    SnapAfterRefreshStickBottom,
}

/// ratatui 0.29：`Paragraph::scroll` 的 `y` 不得大到使内部 `area.height + scroll_y` 溢出；也不得大于「总行数 − 视口行数」。
///
/// 曾对 Snap/Manual 加 35% slack 防「估算低估裁底」，但估算常偏高，slack 会在回答结束后留下大块空白；与流式一致只用严格上限。
pub(super) fn clamped_chat_vertical_scroll(
    text: &str,
    inner_width: u16,
    inner_height: u16,
    mode: ChatVerticalStickMode,
    manual_scroll_y: u16,
) -> u16 {
    let max_scroll = chat_max_scroll_strict(text, inner_width, inner_height);
    match mode {
        ChatVerticalStickMode::Manual => manual_scroll_y.min(max_scroll),
        ChatVerticalStickMode::StreamStickBottom
        | ChatVerticalStickMode::SnapAfterRefreshStickBottom => max_scroll,
    }
}

/// 估算折行下的最大 `scroll_y`（无 slack）。
pub(super) fn chat_max_scroll_strict(text: &str, inner_width: u16, inner_height: u16) -> u16 {
    let rows_base = estimate_wrapped_line_rows(text, inner_width);
    let vis = inner_height.max(1) as usize;
    rows_base.saturating_sub(vis).min(u16::MAX as usize) as u16
}

/// Manual 模式下的滚动上限，供近底 re-pin（与贴底上限一致）。
pub(super) fn chat_manual_max_scroll(text: &str, inner_width: u16, inner_height: u16) -> u16 {
    chat_max_scroll_strict(text, inner_width, inner_height)
}

#[must_use]
pub(super) fn chat_scroll_gap_from_bottom(scroll_y: u16, max_scroll: u16) -> u16 {
    max_scroll.saturating_sub(scroll_y)
}

#[must_use]
pub(super) fn is_near_chat_bottom(gap_rows: u16) -> bool {
    gap_rows <= STICK_NEAR_BOTTOM_ROWS
}

/// 拖滚动条：近底 re-pin；离底超过 unpin 阈值则 unpin（对齐 Web pointer 离底）。
pub(super) fn apply_chat_scrollbar_follow_intent(
    model: &mut TuiModel,
    scroll_y: u16,
    max_scroll: u16,
) {
    model.chat_scroll_y = scroll_y;
    let gap = chat_scroll_gap_from_bottom(scroll_y, max_scroll);
    if is_near_chat_bottom(gap) {
        model.chat_follow_bottom = true;
    } else if gap > STICK_UNPIN_GAP_ROWS {
        model.chat_follow_bottom = false;
    }
}

struct TuiChatPanePrep {
    chat_body: String,
    streaming_nonempty: bool,
}

fn tui_prepare_chat_body_and_stream_flags(
    model: &TuiModel,
    scratch: &TuiLlmStreamScratch,
) -> TuiChatPanePrep {
    let streaming_nonempty = !scratch.reasoning.trim().is_empty()
        || (!scratch.content.trim().is_empty()
            && !model
                .turn_projection
                .should_hide_streaming_content(scratch.content.as_str()));
    let mut transcript_display = model.transcript.clone();
    let projection = model.turn_projection.format_projection_block();
    if !projection.is_empty() {
        transcript_display.push_str("\n\n");
        transcript_display.push_str(projection.as_str());
    }
    if !model.control_plane_tail.is_empty() {
        transcript_display.push_str("\n\n[SSE 控制面]\n");
        transcript_display.push_str(model.control_plane_tail.as_str());
    }
    let chat_body =
        append_tui_streaming_tail(transcript_display.as_str(), scratch, &model.turn_projection);
    TuiChatPanePrep {
        chat_body,
        streaming_nonempty,
    }
}

fn tui_chat_stick_mode_after_snap_clear(
    model: &mut TuiModel,
    streaming_nonempty: bool,
) -> ChatVerticalStickMode {
    let snap_bottom_this_frame = model.chat_snap_bottom_next_draw;
    if snap_bottom_this_frame {
        model.chat_snap_bottom_next_draw = false;
        model.chat_follow_bottom = true;
    }
    if snap_bottom_this_frame {
        ChatVerticalStickMode::SnapAfterRefreshStickBottom
    } else if streaming_nonempty && model.chat_follow_bottom {
        ChatVerticalStickMode::StreamStickBottom
    } else {
        ChatVerticalStickMode::Manual
    }
}

fn maybe_repin_chat_follow_near_bottom(
    model: &mut TuiModel,
    chat_body: &str,
    tw: u16,
    th: u16,
    stick_mode: ChatVerticalStickMode,
) {
    if stick_mode != ChatVerticalStickMode::Manual {
        return;
    }
    let max_scroll = chat_manual_max_scroll(chat_body, tw, th);
    let gap = chat_scroll_gap_from_bottom(model.chat_scroll_y, max_scroll);
    if is_near_chat_bottom(gap) {
        model.chat_follow_bottom = true;
    }
}

fn render_tui_chat_pane(
    frame: &mut Frame<'_>,
    model: &mut TuiModel,
    chat_pane: Rect,
    prep: TuiChatPanePrep,
    color: bool,
) {
    let TuiChatPanePrep {
        chat_body,
        streaming_nonempty,
    } = prep;
    let chat_block = panel_block(" 聊天 ", color, model.focus == TuiFocus::Chat);
    let chat_inner = chat_block_inner_area(chat_pane);
    let (text_rect, scrollbar_rect) = chat_inner_split_text_and_scrollbar(chat_inner);
    let tw = text_rect.width.max(1);
    let th = text_rect.height.max(1);
    let rows_base = estimate_wrapped_line_rows(chat_body.as_str(), tw);
    let vis_lines = th as usize;
    let stick_mode = tui_chat_stick_mode_after_snap_clear(model, streaming_nonempty);
    let chat_scroll_y =
        clamped_chat_vertical_scroll(chat_body.as_str(), tw, th, stick_mode, model.chat_scroll_y);
    model.chat_scroll_y = chat_scroll_y;
    maybe_repin_chat_follow_near_bottom(model, chat_body.as_str(), tw, th, stick_mode);

    frame.render_widget(chat_block, chat_pane);
    let center_body = Paragraph::new(chat_body)
        .wrap(Wrap { trim: false })
        .scroll((chat_scroll_y, 0));
    frame.render_widget(center_body, text_rect);

    if scrollbar_rect.width > 0 && rows_base > vis_lines {
        let bar_style = scrollbar_track_style(color, model.focus == TuiFocus::Chat);
        let max_thumb = rows_base.saturating_sub(vis_lines).min(u16::MAX as usize) as u16;
        let thumb_y = chat_scroll_y.min(max_thumb);
        let mut sb_state =
            ScrollbarState::new(rows_base.saturating_sub(vis_lines).saturating_add(1))
                .position(usize::from(thumb_y))
                .viewport_content_length(vis_lines);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .symbols(scrollbar::VERTICAL)
            .style(bar_style);
        frame.render_stateful_widget(scrollbar, scrollbar_rect, &mut sb_state);
    }
}

fn render_tui_composer_row(
    frame: &mut Frame<'_>,
    composer_pane: Rect,
    model: &TuiModel,
    color: bool,
) {
    let composer_block = panel_block(" 撰写 ", color, model.focus == TuiFocus::Composer);
    let composer_inner = composer_block.inner(composer_pane);
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
    frame.render_widget(input_par, composer_pane);
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
}

fn render_tui_modal_stack(frame: &mut Frame<'_>, area: Rect, model: &TuiModel, color: bool) {
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

pub(super) fn render_full(
    frame: &mut Frame<'_>,
    model: &mut TuiModel,
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

    let scratch_guard = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
    let chat_prep = tui_prepare_chat_body_and_stream_flags(model, &scratch_guard);
    drop(scratch_guard);
    render_tui_chat_pane(frame, model, panes.chat, chat_prep, color);

    render_tui_composer_row(frame, panes.composer, model, color);

    render_side_panel(
        frame,
        panes.side_right,
        " 工作区 ",
        model.right_summary.as_str(),
        color,
        model.focus == TuiFocus::SideRight,
    );

    render_tui_status_bar(frame, vertical[2], model, color);

    render_tui_modal_stack(frame, area, model, color);
}

fn status_run_kind(run: &str) -> &'static str {
    if run.starts_with("错误") {
        "error"
    } else if run.starts_with("工具执行") {
        "tool"
    } else if run.starts_with("模型生成") {
        "running"
    } else {
        "ready"
    }
}

fn status_run_fg(color: bool, kind: &str) -> Color {
    if !color {
        return Color::Reset;
    }
    match kind {
        "error" => Color::LightRed,
        "tool" => Color::Cyan,
        "running" => Color::Yellow,
        _ => Color::LightGreen,
    }
}

fn render_tui_status_bar(frame: &mut Frame<'_>, area: Rect, model: &TuiModel, color: bool) {
    let status_style = status_line_style(color);
    let status_block = Block::default().style(status_style);
    let inner = status_block.inner(area);
    frame.render_widget(status_block, area);
    if inner.width == 0 {
        return;
    }

    let run = model.status_run.as_str();
    let run_cols = (UnicodeWidthStr::width(run) as u16)
        .saturating_add(1)
        .max(4);
    let run_w = run_cols
        .min(inner.width.saturating_div(2).max(4))
        .min(inner.width);
    let chips_w = inner.width.saturating_sub(run_w);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(chips_w), Constraint::Length(run_w)])
        .split(inner);

    let chips_max = chunks[0].width.max(1) as usize;
    let chips_text = truncate_chars_with_ellipsis(model.status_chips.as_str(), chips_max);
    let chips_line = if color {
        Line::from(Span::styled(
            chips_text.as_str(),
            Style::default().fg(Color::White),
        ))
    } else {
        Line::from(chips_text)
    };
    frame.render_widget(Paragraph::new(chips_line), chunks[0]);

    let kind = status_run_kind(run);
    let run_fg = status_run_fg(color, kind);
    let run_line = Line::from(Span::styled(run, Style::default().fg(run_fg)));
    frame.render_widget(
        Paragraph::new(run_line).alignment(Alignment::Right),
        chunks[1],
    );
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
    turn_projection: &TuiTurnProjection,
    control_plane_tail: &str,
    scratch: &TuiLlmStreamScratch,
) -> Option<ChatScrollbarHit> {
    let chat_inner = chat_block_inner_area(chat_pane);
    let (text_rect, sb_rect) = chat_inner_split_text_and_scrollbar(chat_inner);
    if sb_rect.width == 0 {
        return None;
    }
    let mut transcript_display = transcript.to_string();
    let projection_block = turn_projection.format_projection_block();
    if !projection_block.is_empty() {
        transcript_display.push_str("\n\n");
        transcript_display.push_str(projection_block.as_str());
    }
    if !control_plane_tail.is_empty() {
        transcript_display.push_str("\n\n[SSE 控制面]\n");
        transcript_display.push_str(control_plane_tail);
    }
    let chat_body =
        append_tui_streaming_tail(transcript_display.as_str(), scratch, turn_projection);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_bottom_gap_matches_web_rows() {
        assert!(is_near_chat_bottom(0));
        assert!(is_near_chat_bottom(STICK_NEAR_BOTTOM_ROWS));
        assert!(!is_near_chat_bottom(STICK_NEAR_BOTTOM_ROWS + 1));
    }

    #[test]
    fn scrollbar_follow_intent_pins_and_unpins() {
        let mut model = TuiModel {
            header_line: String::new(),
            nav_summary: String::new(),
            right_summary: String::new(),
            transcript: String::new(),
            chat_scroll_y: 0,
            chat_snap_bottom_next_draw: false,
            chat_follow_bottom: true,
            chat_scrollbar_dragging: false,
            input: String::new(),
            status_chips: String::new(),
            status_run: "就绪".into(),
            focus: TuiFocus::default(),
            approval_modal: None,
            approval_backlog: Default::default(),
            clarification_modal: None,
            clarification_backlog: Default::default(),
            workspace_path_buf: std::path::PathBuf::from("."),
            workspace_modal: None,
            sqlite_conversation_id: None,
            control_plane_tail: String::new(),
            turn_projection: TuiTurnProjection::default(),
            committed_turns: Default::default(),
        };
        apply_chat_scrollbar_follow_intent(&mut model, 0, 20);
        assert!(!model.chat_follow_bottom);
        assert_eq!(model.chat_scroll_y, 0);

        apply_chat_scrollbar_follow_intent(&mut model, 19, 20);
        assert!(model.chat_follow_bottom);
        assert_eq!(model.chat_scroll_y, 19);
    }

    #[test]
    fn stick_mode_respects_follow_pin() {
        let mut model = TuiModel {
            header_line: String::new(),
            nav_summary: String::new(),
            right_summary: String::new(),
            transcript: String::new(),
            chat_scroll_y: 0,
            chat_snap_bottom_next_draw: false,
            chat_follow_bottom: false,
            chat_scrollbar_dragging: false,
            input: String::new(),
            status_chips: String::new(),
            status_run: "就绪".into(),
            focus: TuiFocus::default(),
            approval_modal: None,
            approval_backlog: Default::default(),
            clarification_modal: None,
            clarification_backlog: Default::default(),
            workspace_path_buf: std::path::PathBuf::from("."),
            workspace_modal: None,
            sqlite_conversation_id: None,
            control_plane_tail: String::new(),
            turn_projection: TuiTurnProjection::default(),
            committed_turns: Default::default(),
        };
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model, true),
            ChatVerticalStickMode::Manual
        );
        model.chat_follow_bottom = true;
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model, true),
            ChatVerticalStickMode::StreamStickBottom
        );
        model.chat_snap_bottom_next_draw = true;
        model.chat_follow_bottom = false;
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model, true),
            ChatVerticalStickMode::SnapAfterRefreshStickBottom
        );
        assert!(model.chat_follow_bottom);
        assert!(!model.chat_snap_bottom_next_draw);
    }

    #[test]
    fn snap_after_refresh_matches_stream_strict_bottom_no_slack_gap() {
        // 长文：若仍加 35% slack，snap 会显著大于 stream，回答结束后底部出现大空隙。
        let text = "hello world\n".repeat(80);
        let stream_y = clamped_chat_vertical_scroll(
            text.as_str(),
            40,
            10,
            ChatVerticalStickMode::StreamStickBottom,
            0,
        );
        let snap_y = clamped_chat_vertical_scroll(
            text.as_str(),
            40,
            10,
            ChatVerticalStickMode::SnapAfterRefreshStickBottom,
            0,
        );
        let manual_cap = clamped_chat_vertical_scroll(
            text.as_str(),
            40,
            10,
            ChatVerticalStickMode::Manual,
            u16::MAX,
        );
        assert_eq!(stream_y, snap_y);
        assert_eq!(snap_y, manual_cap);
        assert_eq!(snap_y, chat_max_scroll_strict(text.as_str(), 40, 10));
    }

    #[test]
    fn streaming_tail_matches_transcript_assistant_shape() {
        let transcript = "[user]\nhi\n\n";
        let scratch = crate::runtime::tui::TuiLlmStreamScratch {
            reasoning: String::new(),
            content: "你好，世界".into(),
        };
        let projection = TuiTurnProjection::default();
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert!(
            out.starts_with("[user]\nhi\n\n[assistant]\n"),
            "stream must use [assistant] header like final transcript: {out:?}"
        );
        assert!(out.contains("你好，世界"), "stream body missing: {out:?}");
        assert!(
            out.ends_with("\n\n"),
            "stream must end with blank line like messages_to_transcript: {out:?}"
        );
        // 勿在 transcript 已有 `\n\n` 后再多插一空行
        assert!(
            !out.contains("[user]\nhi\n\n\n[assistant]"),
            "extra blank before [assistant] shifts text vs final: {out:?}"
        );
    }
}
