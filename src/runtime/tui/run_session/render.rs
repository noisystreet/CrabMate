//! TUI 分区布局绘制与聊天区滚动估算。

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span, Text};
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
use super::chat_follow::{
    chat_scroll_gap_from_bottom, is_near_chat_bottom, resolve_chat_follow_after_user_scroll,
};
use super::turn_project::TuiTurnProjection;
use super::{TuiFocus, TuiModel};

/// 跟底意图 API（供 `mod.rs` 以 `render::` 调用，实现见 [`super::chat_follow`]）。
pub(super) use super::chat_follow::{
    apply_chat_scrollbar_follow_intent, note_chat_user_scroll_down, note_chat_user_scroll_up,
};

/// 流式尾挂：仅当投影**尚未**拥有 content lane 时附加 `[assistant]\n{body}`。
/// open 段 / 工具相 / 旁白 / 终答由投影（含 live catch-up）承接，避免双显与藏短。
pub(super) fn append_tui_streaming_tail(
    transcript: &str,
    scratch: &crate::runtime::tui::TuiLlmStreamScratch,
    projection: &TuiTurnProjection,
) -> String {
    let r = scratch.reasoning.trim();
    let c = scratch.content.trim();
    let hide_content = projection.owns_streaming_content_lane(scratch);
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

/// 聊天区按行着色（旁白 / 工具 / 终答 / 角色头）；`color=false`（含 `NO_COLOR`）时纯文本。
pub(super) fn chat_body_to_styled_text(text: &str, color: bool) -> Text<'static> {
    if text.is_empty() {
        return Text::default();
    }
    let lines: Vec<Line<'static>> = text
        .split('\n')
        .map(|line| styled_chat_line(line, color))
        .collect();
    Text::from(lines)
}

fn styled_chat_line(line: &str, color: bool) -> Line<'static> {
    let owned = line.to_string();
    if !color {
        return Line::from(owned);
    }
    let style = chat_line_header_style(line);
    match style {
        Some(s) => Line::from(Span::styled(owned, s)),
        None => Line::from(owned),
    }
}

/// 投影/角色标题行样式；正文行返回 `None`。
fn chat_line_header_style(line: &str) -> Option<Style> {
    let t = line.trim_end();
    if t == "[SSE 控制面]" {
        return Some(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    }
    // Tauri 风格工具一行：▸ name  summary
    if t.starts_with('▸') {
        return Some(Style::default().fg(Color::Yellow));
    }
    // 时间线短注
    if t.starts_with('·') && t.chars().nth(1) == Some(' ') {
        return Some(Style::default().fg(Color::Cyan));
    }
    if t == "[user]" {
        return Some(Style::default().fg(Color::Blue));
    }
    if t == "[assistant]" {
        return Some(Style::default().fg(Color::Green));
    }
    if t == "[tool]" {
        return Some(Style::default().fg(Color::Yellow));
    }
    None
}

/// 中区聊天正文唯一合成入口：定稿 transcript + 本轮投影 + 控制面附录 + 流式尾。
pub(super) fn build_tui_chat_body(
    transcript: &str,
    turn_projection: &TuiTurnProjection,
    control_plane_tail: &str,
    scratch: &TuiLlmStreamScratch,
) -> String {
    let mut out = transcript.to_string();
    let projection = turn_projection.format_projection_block(Some(scratch));
    if !projection.is_empty() {
        out.push_str("\n\n");
        out.push_str(projection.as_str());
    }
    if !control_plane_tail.is_empty() {
        out.push_str("\n\n[SSE 控制面]\n");
        out.push_str(control_plane_tail);
    }
    append_tui_streaming_tail(out.as_str(), scratch, turn_projection)
}

struct TuiChatPanePrep {
    chat_body: String,
}

fn tui_prepare_chat_body_and_stream_flags(
    model: &TuiModel,
    scratch: &TuiLlmStreamScratch,
) -> TuiChatPanePrep {
    TuiChatPanePrep {
        chat_body: build_tui_chat_body(
            model.transcript.as_str(),
            &model.turn_projection,
            model.control_plane_tail.as_str(),
            scratch,
        ),
    }
}

fn tui_chat_stick_mode_after_snap_clear(model: &mut TuiModel) -> ChatVerticalStickMode {
    let snap_bottom_this_frame = model.chat_snap_bottom_next_draw;
    if snap_bottom_this_frame {
        model.chat_snap_bottom_next_draw = false;
        model.chat_follow_bottom = true;
    }
    if snap_bottom_this_frame {
        ChatVerticalStickMode::SnapAfterRefreshStickBottom
    } else if model.chat_follow_bottom {
        // 对齐 Web `auto_scroll_chat`：pin 后任意内容增高都贴底（用户消息入列、投影、流式尾），
        // 不依赖 `streaming_nonempty`——否则 Enter 时 snap 常在用户气泡写入前被消费，随后退回 Manual。
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
    // 主动下滑已在 stick 前由 [`resolve_chat_follow_after_user_scroll`] 处理；此处兜底 clamp 后近底。
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
    let TuiChatPanePrep { chat_body } = prep;
    let chat_block = panel_block(" 聊天 ", color, model.focus == TuiFocus::Chat);
    let chat_inner = chat_block_inner_area(chat_pane);
    let (text_rect, scrollbar_rect) = chat_inner_split_text_and_scrollbar(chat_inner);
    let tw = text_rect.width.max(1);
    let th = text_rect.height.max(1);
    let rows_base = estimate_wrapped_line_rows(chat_body.as_str(), tw);
    let vis_lines = th as usize;
    let max_scroll = chat_max_scroll_strict(chat_body.as_str(), tw, th);
    // 须在 stick_mode 之前：下滑 re-pin 后本帧即可 StreamStickBottom
    resolve_chat_follow_after_user_scroll(model, max_scroll);
    let stick_mode = tui_chat_stick_mode_after_snap_clear(model);
    let chat_scroll_y =
        clamped_chat_vertical_scroll(chat_body.as_str(), tw, th, stick_mode, model.chat_scroll_y);
    model.chat_scroll_y = chat_scroll_y;
    maybe_repin_chat_follow_near_bottom(model, chat_body.as_str(), tw, th, stick_mode);

    frame.render_widget(chat_block, chat_pane);
    let center_body = Paragraph::new(chat_body_to_styled_text(chat_body.as_str(), color))
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
    let chat_body = build_tui_chat_body(transcript, turn_projection, control_plane_tail, scratch);
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
    fn stick_mode_respects_follow_pin() {
        let mut model = TuiModel {
            header_line: String::new(),
            nav_summary: String::new(),
            right_summary: String::new(),
            transcript: String::new(),
            chat_scroll_y: 0,
            chat_snap_bottom_next_draw: false,
            chat_follow_bottom: false,
            chat_user_scroll_down: false,
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
            recent_conversations: Vec::new(),
            control_plane_tail: String::new(),
            turn_projection: TuiTurnProjection::default(),
            committed_turns: Default::default(),
        };
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::Manual
        );
        model.chat_follow_bottom = true;
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::StreamStickBottom
        );
        // pin 后即使未流式也要贴底（用户消息入列窗口）
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::StreamStickBottom
        );
        model.chat_snap_bottom_next_draw = true;
        model.chat_follow_bottom = false;
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::SnapAfterRefreshStickBottom
        );
        assert!(model.chat_follow_bottom);
        assert!(!model.chat_snap_bottom_next_draw);
    }

    #[test]
    fn follow_pin_sticks_without_streaming_like_tauri() {
        let mut model = TuiModel {
            header_line: String::new(),
            nav_summary: String::new(),
            right_summary: String::new(),
            transcript: String::new(),
            chat_scroll_y: 0,
            chat_snap_bottom_next_draw: false,
            chat_follow_bottom: true,
            chat_user_scroll_down: false,
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
            recent_conversations: Vec::new(),
            control_plane_tail: String::new(),
            turn_projection: TuiTurnProjection::default(),
            committed_turns: Default::default(),
        };
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::StreamStickBottom
        );
        model.chat_follow_bottom = false;
        assert_eq!(
            tui_chat_stick_mode_after_snap_clear(&mut model),
            ChatVerticalStickMode::Manual
        );
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
    fn build_tui_chat_body_matches_prepare_and_scrollbar_path() {
        let transcript = "[user]\nhi\n\n";
        let scratch = crate::runtime::tui::TuiLlmStreamScratch {
            content: "你好".into(),
            ..Default::default()
        };
        let projection = TuiTurnProjection::default();
        let body = build_tui_chat_body(transcript, &projection, "", &scratch);
        let via_tail = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert_eq!(body, via_tail);
        assert!(body.contains("[assistant]\n你好"), "{body}");
        let with_ctrl = build_tui_chat_body(transcript, &projection, "err line", &scratch);
        assert!(with_ctrl.contains("[SSE 控制面]\nerr line"), "{with_ctrl}");
    }

    #[test]
    fn streaming_tail_keeps_assistant_header() {
        let transcript = "[user]\nhi\n\n";
        let scratch = crate::runtime::tui::TuiLlmStreamScratch {
            reasoning: String::new(),
            content: "你好，世界".into(),
        };
        let projection = TuiTurnProjection::default();
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert!(
            out.starts_with("[user]\nhi\n\n[assistant]\n"),
            "stream must keep [assistant] like final transcript: {out:?}"
        );
        assert!(out.contains("你好，世界"), "stream body missing: {out:?}");
        assert!(
            out.ends_with("\n\n"),
            "stream must end with blank line like messages_to_transcript: {out:?}"
        );
        assert!(
            !out.contains("[user]\nhi\n\n\n[assistant]"),
            "extra blank before [assistant] shifts text vs final: {out:?}"
        );
    }

    #[test]
    fn streaming_tail_still_shows_when_only_timeline_in_projection() {
        // 仅有 intent 时间线时不得误藏正文（曾用「投影非空即 owns」会踩坑）。
        let transcript = "[user]\nhi\n\n";
        let scratch = crate::runtime::tui::TuiLlmStreamScratch {
            content: "正文回答".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "intent_analysis".into(),
                    title: "直接执行".into(),
                    detail: None,
                },
            },
            &scratch,
        );
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert!(
            out.contains("正文回答"),
            "timeline-only projection must not suppress stream body: {out:?}"
        );
        assert!(out.contains("[assistant]\n正文回答"), "{out:?}");
    }

    #[test]
    fn streaming_tail_suppressed_when_projection_owns_content() {
        let transcript = "[user]\nhi\n\n";
        let scratch = crate::runtime::tui::TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            },
            &scratch,
        );
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert_eq!(
            out, transcript,
            "projection commentary must not also stream: {out:?}"
        );
        let block = projection.format_projection_block(Some(&scratch));
        assert!(
            block.contains("[assistant]\n先看一下 README。"),
            "projection must keep [assistant] so the label does not vanish: {block}"
        );
    }

    #[test]
    fn chat_line_headers_get_distinct_styles_when_color_on() {
        assert!(chat_line_header_style("▸ read_file  README.md").is_some());
        assert!(chat_line_header_style("· 意图分析").is_some());
        assert!(chat_line_header_style("[SSE 控制面]").is_some());
        assert!(chat_line_header_style("普通正文").is_none());
        assert!(chat_line_header_style("[旁白]").is_none());
        let text = chat_body_to_styled_text("旁白正文\n▸ read_file  x\n", true);
        assert_eq!(text.lines.len(), 3);
        let plain = chat_body_to_styled_text("▸ tool  hi", false);
        assert_eq!(plain.lines.len(), 1);
        assert!(plain.lines[0].spans[0].style == Style::default());
    }
}
