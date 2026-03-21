//! 布局与绘制（聊天区、右侧面板、弹窗）。

use ratatui::layout::Margin;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Padding;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};
use regex::Regex;
use std::sync::LazyLock;
use tui_markdown::{Options, from_str_with_options as markdown_to_text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use unicodeit::replace as latex_to_unicode;

use crate::types::Message;

use super::state::{Focus, Mode, ModelPhase, RightTab, TuiState};
use super::styles::{
    DarkStyleSheet, HighContrastDarkStyleSheet, HighContrastLightStyleSheet, LightStyleSheet,
    code_themes,
};
use super::text_input;

/// 左右主窗格之间的竖线分隔（独立一列 `│`，不挡内容）。
fn draw_pane_separator_vertical(f: &mut Frame<'_>, col: Rect) {
    if col.width == 0 || col.height == 0 {
        return;
    }
    let style = Style::default().fg(Color::DarkGray);
    let vbar_lines: Vec<Line<'_>> = (0..col.height).map(|_| Line::raw("│")).collect();
    f.render_widget(Paragraph::new(vbar_lines).style(style), col);
}

/// 左侧列：输入区下方「横线 1 行 + 状态栏 1 行」（用于鼠标命中与布局对齐）。
pub(super) const LEFT_COLUMN_ROWS_BELOW_INPUT: u16 = 2;

/// 右侧：标签栏行数（与 `draw_right` 中 `Constraint::Length` 一致；单行 tabs，无上下留白）。
pub(super) const RIGHT_PANEL_TAB_ROWS: u16 = 1;
/// 右侧：标签栏 + 与内容区之间的横线，共占行数（用于鼠标命中与布局对齐）。
pub(super) const RIGHT_PANEL_ROWS_ABOVE_CONTENT: u16 = RIGHT_PANEL_TAB_ROWS + 1;

fn draw_pane_separator_horizontal(f: &mut Frame<'_>, row: Rect) {
    if row.width == 0 || row.height == 0 {
        return;
    }
    let style = Style::default().fg(Color::DarkGray);
    let text = "─".repeat(row.width as usize);
    f.render_widget(Paragraph::new(text).style(style), row);
}

fn right_tab_color(tab: RightTab) -> Color {
    match tab {
        RightTab::Workspace => Color::Green,
        RightTab::Tasks => Color::Yellow,
        RightTab::Schedule => Color::Cyan,
    }
}

/// 与 `draw_chat` 中输入 `Paragraph` 内层一致（光标定位、鼠标落点）。
pub(super) fn chat_input_text_inner(term_cols: u16, term_rows: u16, input_rows: u16) -> Rect {
    let full = Rect::new(0, 0, term_cols, term_rows);
    let hchunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(65),
            Constraint::Length(1),
            Constraint::Percentage(35),
        ])
        .split(full);
    let left = hchunks[0];
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(input_rows.max(2)),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(left);
    vchunks[2].inner(Margin {
        vertical: 1,
        horizontal: 1,
    })
}

/// 状态栏左侧阶段词颜色（与 `ModelPhase::label` 对应）。
fn model_phase_color(phase: ModelPhase) -> Color {
    match phase {
        ModelPhase::Idle => Color::Green,
        ModelPhase::Thinking => Color::Cyan,
        ModelPhase::SelectingTools => Color::Yellow,
        ModelPhase::Answering => Color::Blue,
        ModelPhase::ToolRunning => Color::Rgb(255, 165, 0), // 橙，与「思考」青蓝区分
        ModelPhase::AwaitingApproval => Color::Magenta,
        ModelPhase::Error => Color::Red,
    }
}

pub(super) fn draw_ui(f: &mut Frame<'_>, state: &mut TuiState) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(65),
            Constraint::Length(1),
            Constraint::Percentage(35),
        ])
        .split(area);

    draw_chat(f, chunks[0], state);
    draw_pane_separator_vertical(f, chunks[1]);
    draw_right(f, chunks[2], &*state);

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
                "工具审批（命令 / http_fetch 等）",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::raw(format!("标识: {}", state.pending_command)),
            Line::raw(format!("详情: {}", args_text)),
            Line::raw(""),
            Line::from(option_line),
            Line::raw("←/→ 选择，Enter 确认（1/2/3 选项，Esc=拒绝）"),
            Line::raw("快捷键：D=拒绝，O=本次允许，P=永久允许（按下即确认）"),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
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
            Line::from(
                "布局：左侧对话与输入区以横线分隔，左右主区域以竖线分隔；右侧为 工作区 / 任务 / 日程 标签页。",
            ),
            Line::from("焦点切换：F2 在 聊天 和 右侧 面板之间切换，Tab 在右侧标签页间切换。"),
            Line::from(
                "发送：Enter 发送；Shift+Enter 换行。←→ 移动光标、↑↓ 按显示行移动、Home/End 行首行尾、Delete 向后删。",
            ),
            Line::from(
                "剪贴板：Ctrl+V 从系统剪贴板粘贴（Linux 需 X11/Wayland 与剪贴板环境；失败时静默跳过）。",
            ),
            Line::from("制表符：Tab 在右侧标签间切换；Ctrl+Tab 或 Ctrl+I 在输入中插入 Tab 字符。"),
            Line::from(
                "撤销/重做：Ctrl+Z 撤销，Ctrl+Y 或 Ctrl+Shift+Z 重做（多行编辑）。折行与显示可能与 Markdown 区略有偏差，属预期范围。",
            ),
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

/// TUI 已单独画一行「模型:」；正文里常见 `模型：…`、`## 模型：`、`**模型：**` 等重复标签，用正则循环剥掉。
static ASSISTANT_LEADING_ROLE_ECHO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^[\s\u{feff}\u{3000}]*
        (?:
            (?: \#+ | > ) \s*
            (模型|助手|Assistant|Model)
            \s* [：:]
          | \*{1,2} \s* (模型|助手|Assistant|Model) \s* [：:] \s* \*{1,2}
          | _{1,2} \s* (模型|助手|Assistant|Model) \s* [：:] \s* _{1,2}
          | 【 \s* 模型 \s* 】 \s* [：:]
          | (模型|助手|Assistant|Model) \s* [：:]
        )
        \s*",
    )
    .expect("ASSISTANT_LEADING_ROLE_ECHO")
});

/// 整行只有「角色称呼」时（含 `# 模型：`、`**模型：**` 等），与 TUI 顶栏「模型:」重复，应剥掉。
static STANDALONE_ROLE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x) ^ \s*
        (?: \#+ \s* )?
        (?: > \s* )?
        (?: \*{1,2} | _{1,2} )? \s*
        (?: 【 \s* 模型 \s* 】 \s* [：:] | (模型|助手|Assistant|Model) \s* [：:] )
        \s*
        (?: \*{1,2} | _{1,2} )? \s*
        $",
    )
    .expect("STANDALONE_ROLE_LINE")
});

fn is_standalone_role_echo_line(t: &str) -> bool {
    let t = t.trim().trim_matches('\u{3000}');
    if t.is_empty() {
        return false;
    }
    matches!(
        t,
        "模型"
            | "模型："
            | "模型:"
            | "Assistant"
            | "Assistant："
            | "Assistant:"
            | "助手"
            | "助手："
            | "助手:"
            | "Model"
            | "Model："
            | "Model:"
    ) || STANDALONE_ROLE_LINE.is_match(t)
}

fn strip_leading_blank_and_role_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim().trim_matches('\u{3000}');
        if t.is_empty() || is_standalone_role_echo_line(t) {
            i += 1;
            continue;
        }
        break;
    }
    lines[i..].join("\n")
}

fn strip_assistant_echo_label(content: &str) -> String {
    let mut s = content
        .trim_start()
        .trim_start_matches('\u{feff}')
        .to_string();
    for _ in 0..32 {
        let before = s.clone();
        // 1) 字符串开头的「模型：」块（含 Markdown 前缀）
        for _ in 0..12 {
            let trimmed = s.trim_start().trim_start_matches('\u{feff}');
            let next = ASSISTANT_LEADING_ROLE_ECHO.replace(trimmed, "");
            let next = next.trim_start().trim_start_matches('\u{feff}').to_string();
            if next == s {
                break;
            }
            s = next;
        }
        // 2) 前导空行 + 单独一行的「模型：」（API 常见：\n\n模型：\n正文）
        s = strip_leading_blank_and_role_lines(&s);
        if s == before {
            break;
        }
    }
    s
}

/// 按终端显示宽度截断（宽字符计列宽），超出加省略号。
fn truncate_display_width(s: &str, max_w: usize) -> String {
    if max_w == 0 {
        return String::new();
    }
    if s.width() <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > max_w.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    if out.width() < s.width() {
        out.push('…');
    }
    out
}

fn draw_chat(f: &mut Frame<'_>, area: Rect, state: &mut TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1), // 聊天区下横线
            Constraint::Length(state.input_rows.max(2)),
            Constraint::Length(1), // 输入区与状态栏之间横线
            Constraint::Length(1), // 状态栏（单行文字）
        ])
        .split(area);

    let mut lines: Vec<Line<'_>> = Vec::new();
    let chat_inner_width = vchunks[0].width.saturating_sub(2) as usize;
    let chat_msgs: Vec<&Message> = state
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .collect();
    let rendered_list: Vec<String> = chat_msgs
        .iter()
        .map(|m| {
            let raw = m.content.as_deref().unwrap_or("");
            let display_raw = if m.role == "tool" {
                serde_json::from_str::<serde_json::Value>(raw)
                    .ok()
                    .and_then(|v| {
                        v.get("human_summary")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| raw.to_string())
            } else if m.role == "assistant" {
                strip_assistant_echo_label(raw)
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
        } else if m.role != "assistant" {
            // tool 等仍显示标签；assistant 不画「模型:」，避免与正文里的「模型：」叠成两行。
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
                // 仅用户气泡内正文保持靠右；模型回复与工具输出一律左对齐。
                let line_str = if m.role == "user" {
                    if l.width() >= chat_inner_width {
                        l.to_string()
                    } else {
                        format!(
                            "{}{}",
                            " ".repeat(chat_inner_width.saturating_sub(l.width())),
                            l
                        )
                    }
                } else {
                    l.to_string()
                };
                lines.push(Line::raw(line_str));
            }
        }
        lines.push(Line::raw(""));
    }
    let chat_height = vchunks[0].height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    if chat_height > 0 && !lines.is_empty() {
        let max_start = total_lines.saturating_sub(chat_height);
        if state.chat_follow_tail {
            state.chat_first_line = max_start;
        } else {
            state.chat_first_line = state.chat_first_line.min(max_start);
            // 滚到最底一行后自动恢复「跟随尾部」，便于继续看流式输出
            if max_start > 0 && state.chat_first_line >= max_start {
                state.chat_follow_tail = true;
            }
        }
        let start = state.chat_first_line.min(max_start);
        state.chat_first_line = start;
        let end = (start + chat_height).min(total_lines);
        lines = lines[start..end].to_vec();
    } else {
        state.chat_first_line = 0;
    }
    let chat_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1));
    let chat = Paragraph::new(lines)
        .block(chat_block)
        .wrap(Wrap { trim: false });
    f.render_widget(chat, vchunks[0]);
    draw_pane_separator_horizontal(f, vchunks[1]);

    let input_text = if state.mode == Mode::Prompt {
        state.prompt.as_str()
    } else {
        state.input.as_str()
    };
    let input_focused = state.mode == Mode::Prompt || state.focus == Focus::ChatInput;
    let input_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::symmetric(1, 1));
    let input = Paragraph::new(input_text)
        .block(input_block)
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false });
    f.render_widget(input, vchunks[2]);
    draw_pane_separator_horizontal(f, vchunks[3]);

    if state.mode != Mode::CommandApprove && !state.show_help && input_focused {
        let inner = vchunks[2].inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
        if inner.width > 0 && inner.height > 0 {
            let mw = inner.width.max(1) as usize;
            let cur = if state.mode == Mode::Prompt {
                state.prompt_cursor
            } else {
                state.input_cursor
            };
            let (row, col_w) = text_input::coords_before_cursor(input_text, cur, mw);
            let x = inner
                .x
                .saturating_add((col_w as u16).min(inner.width.saturating_sub(1)));
            let max_row = inner.height.saturating_sub(1);
            let y = inner.y.saturating_add(row.min(max_row));
            f.set_cursor_position((x, y));
        }
    }

    // 底栏：`vchunks[3]` 为横线（见上 `draw_pane_separator_horizontal`），状态文字在下一行 `vchunks[4]`
    let status_rect = vchunks[4];
    let inner_cols = status_rect.width.max(1) as usize;
    let phase = state.model_phase;
    let phase_label = phase.label();
    let phase_color = model_phase_color(phase);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let meta = state.status_line.trim();
    let meta = if meta.is_empty() {
        "Ctrl+C 退出  F1 帮助"
    } else {
        meta
    };
    let meta = meta.replace(['\n', '\r'], " ");
    // 「 ␣阶段 │ 」占宽，右侧说明单独按列宽截断，避免整串截断吃掉彩色阶段词
    let prefix_w = 1usize.saturating_add(phase_label.width()).saturating_add(3); // " │ "
    let meta_max = inner_cols.saturating_sub(prefix_w).max(1);
    let bar_meta = truncate_display_width(&meta, meta_max);
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" ", bold),
        Span::styled(
            phase_label,
            Style::default()
                .fg(phase_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", bold),
        Span::styled(bar_meta, bold),
    ]));
    f.render_widget(status, status_rect);
}

fn draw_right(f: &mut Frame<'_>, area: Rect, state: &TuiState) {
    let vchunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(RIGHT_PANEL_TAB_ROWS),
            Constraint::Length(1), // 标签栏与内容区横线
            Constraint::Min(3),
        ])
        .split(area);

    let titles: Vec<Line> = RightTab::titles()
        .iter()
        .map(|t| Line::from(Span::raw(*t)))
        .collect();
    let tabs_block = Block::default()
        .borders(Borders::NONE)
        .padding(Padding::horizontal(1));
    let tabs = Tabs::new(titles)
        .select(state.tab as usize)
        .block(tabs_block)
        .highlight_style(
            Style::default()
                .fg(right_tab_color(state.tab))
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    f.render_widget(tabs, vchunks[0]);
    draw_pane_separator_horizontal(f, vchunks[1]);

    match state.tab {
        RightTab::Workspace => {
            let mut lines = Vec::new();
            lines.push(Line::raw(format!(
                "根目录：{}",
                state.workspace_dir.display()
            )));
            lines.push(Line::raw(
                "快捷键：F2 聚焦 | Enter 打开/进入 | Backspace 上级 | ↑↓ 选择 | r 刷新",
            ));
            lines.push(Line::raw(""));
            for (i, (name, is_dir)) in state.workspace_entries.iter().enumerate().take(200) {
                let prefix = if *is_dir { "[D]" } else { "   " };
                let s = format!("{} {}", prefix, name);
                if i == state.workspace_sel {
                    lines.push(Line::from(Span::styled(
                        s,
                        Style::default().add_modifier(Modifier::REVERSED),
                    )));
                } else {
                    lines.push(Line::raw(s));
                }
            }
            let workspace_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1));
            let w = Paragraph::new(lines)
                .block(workspace_block)
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[2]);
        }
        RightTab::Tasks => {
            let mut lines = Vec::new();
            lines.push(Line::raw(
                "快捷键：F2 聚焦 | Space 勾选/取消 | ↑↓ 选择 | r 刷新",
            ));
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
            let tasks_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1));
            let w = Paragraph::new(lines)
                .block(tasks_block)
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[2]);
        }
        RightTab::Schedule => {
            let mut lines = Vec::new();
            lines.push(Line::raw(
                "快捷键：F2 聚焦 | t=提醒 e=日程 | Space 完成/取消提醒 | a 新增提醒 | ↑↓ 选择 | r 刷新",
            ));
            lines.push(Line::raw(""));
            let sub_title = if state.schedule_sub == 0 {
                "提醒"
            } else {
                "日程"
            };
            lines.push(Line::from(Span::styled(
                format!("当前：{}", sub_title),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));

            if state.schedule_sub == 0 {
                if state.reminder_items.is_empty() {
                    lines.push(Line::raw("（无提醒）"));
                } else {
                    for (i, (_id, title, done, due_at)) in
                        state.reminder_items.iter().enumerate().take(50)
                    {
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
            let schedule_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1));
            let w = Paragraph::new(lines)
                .block(schedule_block)
                .wrap(Wrap { trim: false });
            f.render_widget(w, vchunks[2]);
        }
    }

    if state.mode == Mode::FileView {
        let block = Block::default()
            .borders(Borders::NONE)
            .padding(Padding::symmetric(1, 1));
        let title = format!("查看文件（Esc/q 关闭）：{}", state.file_view_title);
        let full = format!("{}\n{}\n", title, state.file_view_content);
        let content = Paragraph::new(full).block(block).wrap(Wrap { trim: false });
        f.render_widget(content, vchunks[2]);
    }
}
