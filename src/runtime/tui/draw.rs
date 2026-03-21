//! 布局与绘制（聊天区、右侧面板、弹窗）。

use ratatui::layout::Margin;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};
use ratatui::widgets::Padding;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};
use tui_markdown::{from_str_with_options as markdown_to_text, Options};
use unicode_width::UnicodeWidthStr;
use unicodeit::replace as latex_to_unicode;

use crate::types::Message;

use super::state::{Focus, Mode, RightTab, TuiState};
use super::styles::{
    code_themes, DarkStyleSheet, HighContrastDarkStyleSheet, HighContrastLightStyleSheet,
    LightStyleSheet,
};

fn draw_rect_corners(
    f: &mut Frame<'_>,
    rect: Rect,
    tl: &'static str,
    tr: &'static str,
    bl: &'static str,
    br: &'static str,
    style: Style,
) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let buf = f.buffer_mut();
    let x0 = rect.x;
    let x1 = rect.x + rect.width.saturating_sub(1);
    let y0 = rect.y;
    let y1 = rect.y + rect.height.saturating_sub(1);

    if let Some(cell) = buf.cell_mut((x0, y0)) {
        cell.set_symbol(tl);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x1, y0)) {
        cell.set_symbol(tr);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x0, y1)) {
        cell.set_symbol(bl);
        cell.set_style(style);
    }
    if let Some(cell) = buf.cell_mut((x1, y1)) {
        cell.set_symbol(br);
        cell.set_style(style);
    }
}

fn right_tab_color(tab: RightTab) -> Color {
    match tab {
        RightTab::Workspace => Color::Green,
        RightTab::Tasks => Color::Yellow,
        RightTab::Schedule => Color::Cyan,
    }
}

pub(super) fn draw_ui(f: &mut Frame<'_>, state: &TuiState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    draw_chat(f, chunks[0], state);
    draw_right(f, chunks[1], state);

    const SHOW_SEPARATORS: bool = false;
    if SHOW_SEPARATORS {
        let sep_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD);

        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(state.input_rows.max(2)),
                Constraint::Length(1),
            ])
            .split(chunks[0]);

        let left_sep1_y = left_chunks[1].y;
        for dy in 0..2u16 {
            let y = left_sep1_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[0].x, y, chunks[0].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[0].width as usize)).style(sep_style),
                sep_area,
            );
        }

        let left_sep2_y = left_chunks[2].y;
        for dy in 0..2u16 {
            let y = left_sep2_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[0].x, y, chunks[0].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[0].width as usize)).style(sep_style),
                sep_area,
            );
        }

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(chunks[1]);

        let right_sep_y = right_chunks[1].y;
        for dy in 0..2u16 {
            let y = right_sep_y.saturating_add(dy);
            if y >= area.y.saturating_add(area.height) {
                continue;
            }
            let sep_area = Rect::new(chunks[1].x, y, chunks[1].width, 1);
            f.render_widget(Clear, sep_area);
            f.render_widget(
                Paragraph::new("━".repeat(chunks[1].width as usize)).style(sep_style),
                sep_area,
            );
        }

        let separator_x_start = chunks[1].x.saturating_sub(1);
        for dx in 0..2u16 {
            let x = separator_x_start.saturating_add(dx);
            if x >= area.x.saturating_add(area.width) {
                continue;
            }
            let sep_area = Rect::new(x, area.y, 1, area.height);
            f.render_widget(Clear, sep_area);
            let vbar_lines: Vec<Line<'_>> =
                (0..sep_area.height).map(|_| Line::raw("┃")).collect();
            f.render_widget(Paragraph::new(vbar_lines).style(sep_style), sep_area);
        }
    }

    if state.mode == Mode::CommandApprove {
        let w = area.width.saturating_mul(7) / 10;
        let h: u16 = 9;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let popup = Rect::new(x, y, w.max(50), h);
        let options = ["拒绝", "本次允许", "永久允许"];
        let mut option_line: Vec<Span<'_>> = Vec::new();
        for (i, text) in options.iter().enumerate() {
            if i as u8 == state.approve_choice {
                option_line.push(Span::styled(
                    format!("[{}]", text),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ));
            } else {
                option_line.push(Span::raw(format!(" {} ", text)));
            }
            option_line.push(Span::raw("  "));
        }
        let args_text = if state.pending_command_args.trim().is_empty() {
            "(无参数)".to_string()
        } else {
            state.pending_command_args.clone()
        };
        let lines = vec![
            Line::from(Span::styled(
                "命令执行审批",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(format!("命令: {}", state.pending_command)),
            Line::raw(format!("参数: {}", args_text)),
            Line::raw(""),
            Line::from(option_line),
            Line::raw("←/→ 选择，Enter 确认（1/2/3 选项，Esc=拒绝）"),
            Line::raw("快捷键：D=拒绝，O=本次允许，P=永久允许（按下即确认）"),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .title(" 命令审批 ");
        let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        f.render_widget(Clear, popup);
        f.render_widget(para, popup);
    }

    if state.show_help {
        let w = area.width.saturating_mul(7) / 10;
        let h = area.height.saturating_mul(8) / 10;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let popup = Rect::new(x, y, w.max(40), h.max(15));

        let help_lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Crabmate TUI 小教程",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from("布局：左侧对话 + 输入区域，右侧 工作区 / 任务 / 日程 标签页。"),
            Line::from("焦点切换：F2 在 聊天 和 右侧 面板之间切换，Tab 在右侧标签页间切换。"),
            Line::from("发送：在输入框中按 Enter 发送消息；工具运行时状态栏会提示。"),
            Line::from("Markdown：F3 切换代码主题，F4 切换 Markdown 暗/亮样式。"),
            Line::from("高对比度：F5 在普通 / 高对比度模式之间切换（适合弱光/弱视）。"),
            Line::from("任务 / 日程：右侧标签页中查看和勾选任务、提醒和事件。"),
            Line::raw(""),
            Line::from("按 F1 或 Esc 关闭此帮助，随时再次按 F1 重新查看。"),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .title(" 帮助 / 教程 ");
        let para = Paragraph::new(help_lines)
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(Clear, popup);
        f.render_widget(para, popup);
    }
}

fn strip_assistant_echo_label(content: &str) -> &str {
    let s = content.trim_start();
    for p in ["模型：", "模型:", "Assistant：", "Assistant:", "助手：", "助手:"] {
        if let Some(rest) = s.strip_prefix(p) {
            return rest.trim_start();
        }
    }
    if let Some((first, rest)) = s.split_once('\n') {
        let t = first.trim();
        if matches!(
            t,
            "模型" | "模型：" | "模型:" | "Assistant" | "Assistant：" | "Assistant:" | "助手" | "助手：" | "助手:"
        ) {
            return rest.trim_start();
        }
    }
    s
}

fn draw_chat(f: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(state.input_rows.max(2)),
            Constraint::Length(1),
        ])
        .split(area);

    let mut lines: Vec<Line<'_>> = Vec::new();
    let chat_inner_width = vchunks[0].width.saturating_sub(2) as usize;
    let chat_msgs: Vec<&Message> = state.messages.iter().filter(|m| m.role != "system").collect();
    let rendered_list: Vec<String> = chat_msgs
        .iter()
        .map(|m| {
            let raw = m.content.as_deref().unwrap_or("");
            let display_raw = if m.role == "tool" {
                serde_json::from_str::<serde_json::Value>(raw)
                    .ok()
                    .and_then(|v| v.get("human_summary").and_then(|x| x.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| raw.to_string())
            } else if m.role == "assistant" {
                strip_assistant_echo_label(raw).to_string()
            } else {
                raw.to_string()
            };
            latex_to_unicode(&display_raw)
        })
        .collect();

    for (idx, m) in chat_msgs.iter().enumerate() {
        let role = if m.role == "user" { "我" } else { "模型" };
        let rendered = rendered_list[idx].as_str();
        if m.role == "user" {
            let role_text = format!("{}:", role);
            let role_padded = if role_text.width() >= chat_inner_width {
                role_text
            } else {
                format!(
                    "{}{}",
                    " ".repeat(chat_inner_width.saturating_sub(role_text.width())),
                    role_text
                )
            };
            lines.push(Line::from(Span::styled(
                role_padded,
                Style::default().add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{}:", role),
                Style::default().add_modifier(Modifier::BOLD),
            )));
        }
        if m.role == "assistant" {
            let theme = code_themes()[state.code_theme_idx];
            let text = match (state.md_style, state.high_contrast) {
                (0, false) => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (0, true) => {
                    let options = Options::new(HighContrastDarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (1, false) => {
                    let options = Options::new(LightStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                (1, true) => {
                    let options = Options::new(HighContrastLightStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
                _ => {
                    let options = Options::new(DarkStyleSheet).with_code_theme(theme);
                    markdown_to_text(rendered, &options)
                }
            };
            lines.extend(text.lines.into_iter());
        } else {
            for l in rendered.lines() {
                let padded = if l.width() >= chat_inner_width {
                    l.to_string()
                } else {
                    format!(
                        "{}{}",
                        " ".repeat(chat_inner_width.saturating_sub(l.width())),
                        l
                    )
                };
                lines.push(Line::raw(padded));
            }
        }
        lines.push(Line::raw(""));
    }
    let chat_height = vchunks[0].height.saturating_sub(2);
    if chat_height > 0 && !lines.is_empty() {
        let total = lines.len() as i32;
        let height = chat_height as i32;
        let max_offset = (total - height).max(0);
        let offset_from_bottom = state.chat_scroll.clamp(0, max_offset);
        let start = (max_offset - offset_from_bottom) as usize;
        let end = (start as i32 + height).min(total) as usize;
        lines = lines[start..end].to_vec();
    }
    let chat_focused = state.focus == Focus::ChatView;
    let chat_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(Color::Black));
    let chat = Paragraph::new(lines)
        .block(chat_block)
        .wrap(Wrap { trim: false });
    f.render_widget(chat, vchunks[0]);
    let chat_corner_style = Style::default()
        .fg(if chat_focused { Color::Cyan } else { Color::DarkGray })
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[0],
        "┏",
        "┓",
        "┗",
        "┛",
        chat_corner_style,
    );

    let input_text = if state.mode == Mode::Prompt {
        state.prompt.as_str()
    } else {
        state.input.as_str()
    };
    let input_focused = state.mode == Mode::Prompt || state.focus == Focus::ChatInput;
    let input_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(Color::DarkGray));
    let input = Paragraph::new(input_text)
        .block(input_block)
        .style(Style::default().fg(Color::Gray).bg(Color::DarkGray))
        .wrap(Wrap { trim: false });
    f.render_widget(input, vchunks[1]);

    let input_corner_style = Style::default()
        .fg(if input_focused { Color::Yellow } else { Color::DarkGray })
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[1],
        "┏",
        "┓",
        "┗",
        "┛",
        input_corner_style,
    );

    if state.mode != Mode::CommandApprove && !state.show_help {
        if let Some((mx, my)) = state.cursor_mouse_pos {
            let area = f.area();
            let max_x = area.x.saturating_add(area.width.saturating_sub(1));
            let max_y = area.y.saturating_add(area.height.saturating_sub(1));
            let x = mx.min(max_x);
            let y = my.min(max_y);
            f.set_cursor_position((x, y));
        } else if input_focused {
            let inner = vchunks[1].inner(Margin { vertical: 1, horizontal: 1 });
            if inner.width > 0 && inner.height > 0 {
                if let Some((cx, cy)) = state.cursor_override {
                    let rel_x = cx.saturating_sub(inner.x);
                    let rel_y = cy.saturating_sub(inner.y);
                    let max_dx = inner.width.saturating_sub(1);
                    let max_dy = inner.height.saturating_sub(1);
                    let x = inner.x.saturating_add(rel_x.min(max_dx));
                    let y = inner.y.saturating_add(rel_y.min(max_dy));
                    f.set_cursor_position((x, y));
                } else {
                    let lines: Vec<&str> = input_text.split('\n').collect();
                    let line_idx = lines.len().saturating_sub(1);
                    let last = lines.get(line_idx).copied().unwrap_or("");
                    let x_off = last.width() as u16;
                    let x = inner
                        .x
                        .saturating_add(x_off)
                        .min(inner.x + inner.width.saturating_sub(1));
                    let y = inner
                        .y
                        .saturating_add(line_idx as u16)
                        .min(inner.y + inner.height.saturating_sub(1));
                    f.set_cursor_position((x, y));
                }
            }
        }
    }

    let status_color = match state.focus {
        Focus::ChatView => Color::Cyan,
        Focus::ChatInput => Color::Yellow,
        Focus::Workspace => Color::Green,
        Focus::Right => Color::Magenta,
    };
    let status_bg = Color::Blue;
    let status_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(status_bg));
    let status = Paragraph::new(state.status_line.as_str())
        .block(status_block)
        .style(Style::default().fg(status_color).bg(status_bg));
    f.render_widget(status, vchunks[2]);
    let status_corner_style = Style::default()
        .fg(status_color)
        .add_modifier(Modifier::BOLD);
    draw_rect_corners(
        f,
        vchunks[2],
        "┏",
        "┓",
        "┗",
        "┛",
        status_corner_style,
    );
}

fn draw_right(f: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3)])
        .split(area);

    let titles: Vec<Line> = RightTab::titles()
        .iter()
        .map(|t| Line::from(Span::raw(*t)))
        .collect();
    let right_focused = state.focus == Focus::Right;
    let tabs_bg = Color::DarkGray;
    let tabs_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1))
        .style(Style::default().bg(tabs_bg));
    let tabs = Tabs::new(titles)
        .select(state.tab as usize)
        .block(tabs_block)
        .highlight_style(
            Style::default()
                .fg(right_tab_color(state.tab))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    f.render_widget(tabs, vchunks[0]);
    let tabs_corner_color = if right_focused {
        right_tab_color(state.tab)
    } else {
        Color::DarkGray
    };
    draw_rect_corners(
        f,
        vchunks[0],
        "┏",
        "┓",
        "┗",
        "┛",
        Style::default()
            .fg(tabs_corner_color)
            .add_modifier(Modifier::BOLD),
    );

    match state.tab {
        RightTab::Workspace => {
            let mut lines = Vec::new();
            lines.push(Line::raw(format!("根目录：{}", state.workspace_dir.display())));
            lines.push(Line::raw("快捷键：F2 聚焦 | Enter 打开/进入 | Backspace 上级 | ↑↓ 选择 | r 刷新"));
            lines.push(Line::raw(""));
            for (i, (name, is_dir)) in state.workspace_entries.iter().enumerate().take(200) {
                let prefix = if *is_dir { "[D]" } else { "   " };
                let s = format!("{} {}", prefix, name);
                if i == state.workspace_sel {
                    lines.push(Line::from(Span::styled(s, Style::default().add_modifier(Modifier::REVERSED))));
                } else {
                    lines.push(Line::raw(s));
                }
            }
            let workspace_focused = state.focus == Focus::Workspace;
            let workspace_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Black));
            let w = Paragraph::new(lines)
                .block(workspace_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if workspace_focused {
                Color::Green
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
        RightTab::Tasks => {
            let mut lines = Vec::new();
            lines.push(Line::raw("快捷键：F2 聚焦 | Space 勾选/取消 | ↑↓ 选择 | r 刷新"));
            lines.push(Line::raw(""));
            if state.task_items.is_empty() {
                lines.push(Line::raw("tasks.json 不存在或为空。"));
            } else {
                for (i, (_id, title, done)) in state.task_items.iter().enumerate().take(200) {
                    let s = format!("[{}] {}", if *done { "✓" } else { " " }, title);
                    if state.focus == Focus::Right && i == state.task_sel {
                        lines.push(Line::from(Span::styled(
                            s,
                            Style::default().add_modifier(Modifier::REVERSED),
                        )));
                    } else {
                        lines.push(Line::raw(s));
                    }
                }
            }
            let tasks_focused = state.focus == Focus::Right && state.tab == RightTab::Tasks;
            let tasks_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Blue));
            let w = Paragraph::new(lines)
                .block(tasks_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if tasks_focused {
                Color::Yellow
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
        RightTab::Schedule => {
            let mut lines = Vec::new();
            lines.push(Line::raw(
                "快捷键：F2 聚焦 | t=提醒 e=日程 | Space 完成/取消提醒 | a 新增提醒 | ↑↓ 选择 | r 刷新",
            ));
            lines.push(Line::raw(""));
            let sub_title = if state.schedule_sub == 0 { "提醒" } else { "日程" };
            lines.push(Line::from(Span::styled(
                format!("当前：{}", sub_title),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));

            if state.schedule_sub == 0 {
                if state.reminder_items.is_empty() {
                    lines.push(Line::raw("（无提醒）"));
                } else {
                    for (i, (_id, title, done, due_at)) in state.reminder_items.iter().enumerate().take(50) {
                        let mut s = format!("[{}] {}", if *done { "✓" } else { " " }, title);
                        if due_at.is_some() {
                            s.push_str(" (有到期时间)");
                        }
                        if state.focus == Focus::Right && i == state.reminder_sel {
                            lines.push(Line::from(Span::styled(
                                s,
                                Style::default().add_modifier(Modifier::REVERSED),
                            )));
                        } else {
                            lines.push(Line::raw(s));
                        }
                    }
                }
            } else if state.event_items.is_empty() {
                lines.push(Line::raw("（无日程）"));
            } else {
                for (i, (_id, title, start_at)) in state.event_items.iter().enumerate().take(50) {
                    let s = if start_at.is_empty() {
                        title.clone()
                    } else {
                        format!("{} (有开始时间)", title)
                    };
                    if state.focus == Focus::Right && i == state.event_sel {
                        lines.push(Line::from(Span::styled(
                            s,
                            Style::default().add_modifier(Modifier::REVERSED),
                        )));
                    } else {
                        lines.push(Line::raw(s));
                    }
                }
            }
            let schedule_focused = state.focus == Focus::Right && state.tab == RightTab::Schedule;
            let schedule_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1))
                .style(Style::default().bg(Color::Magenta));
            let w = Paragraph::new(lines)
                .block(schedule_block)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[1]);
            let c = if schedule_focused {
                Color::Cyan
            } else {
                Color::DarkGray
            };
            draw_rect_corners(
                f,
                vchunks[1],
                "┏",
                "┓",
                "┗",
                "┛",
                Style::default().fg(c).add_modifier(Modifier::BOLD),
            );
        }
    }

    if state.mode == Mode::FileView {
        let block = Block::default()
            .borders(Borders::NONE)
            .padding(Padding::symmetric(1, 1))
            .style(Style::default().bg(Color::DarkGray));
        let title = format!("查看文件（Esc/q 关闭）：{}", state.file_view_title);
        let full = format!("{}\n{}\n", title, state.file_view_content);
        let content = Paragraph::new(full)
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(content, vchunks[1]);
        draw_rect_corners(
            f,
            vchunks[1],
            "┏",
            "┓",
            "┗",
            "┛",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
        );
    }
}
