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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tui_markdown::{Options, from_str_with_options as markdown_to_text};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::runtime::latex_unicode::latex_math_to_unicode;
use crate::runtime::message_display::{
    assistant_markdown_source_for_display, tool_content_for_display,
};
use crate::runtime::plan_section::STAGED_PLAN_SECTION_HEADER;
use crate::types::Message;

use super::state::{ChatMessageLineCacheEntry, Focus, Mode, ModelPhase, RightTab, TuiState};
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
        RightTab::Queue => Color::Magenta,
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
        let popup = Rect::new(x, y, w.max(40), h.max(22));

        let help_lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "Crabmate TUI · 完整键位表",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "（底栏左侧为阶段词；完整键位见下文）",
                Style::default().fg(Color::DarkGray),
            )),
            Line::raw(""),
            Line::from(
                "【全局】Ctrl+C 退出。F1 开关本页。F10 打开运行状况（与 GET /health 同逻辑），Esc 或 F10 关闭。Esc 另可关闭文件预览、提示行等（见各模式说明）。",
            ),
            Line::from("【焦点 F2】循环：聊天视图 → 聊天输入 → 工作区列表 → 右侧面板 → 聊天视图。"),
            Line::from(
                "【右侧】Tab 在工作区 / 队列 / 任务 / 日程 标签间切换。鼠标：可点标签与列表；在输入区外松开左键会按落点切换焦点/标签（与 F2 配合）。",
            ),
            Line::from(
                "【聊天输入·键盘】Enter 发送消息。Shift+Enter 在输入框内换行（多行编辑）。←→↑↓ 移动光标（↑↓ 按折行后显示行）；Home/End 行首/行尾；Backspace、Delete。PgUp/PgDn 上下翻动聊天区。",
            ),
            Line::from(
                "【聊天输入·鼠标】在输入框内点击可定位光标（含提示行模式）。鼠标在左侧聊天区滚轮：向上/向下滚动历史。在「输入框与状态栏之间的横线」按住左键拖拽：调节输入区高度（约 3～12 行）。",
            ),
            Line::from(
                "【剪贴板与 Tab】Ctrl+V 粘贴（Linux 需剪贴板环境；失败则静默跳过）。未按 Ctrl 时 Tab 在右侧三个标签间循环。在聊天输入中插入制表符：Ctrl+Tab 或 Ctrl+I。",
            ),
            Line::from("【撤销】Ctrl+Z 撤销，Ctrl+Y 或 Ctrl+Shift+Z 重做（聊天输入与提示行内）。"),
            Line::from(
                "【搜索 / 跳转】Ctrl+F 或 F6（无搜索结果时）打开关键词搜索，Enter 执行；有结果时 F6 下一处、Shift+F6 上一处。F7 打开「按序号跳转」（可见消息从 1 起，不含系统提示）。",
            ),
            Line::from(
                "【导出 / 会话 / 健康】F8 导出 JSON、F9 导出 Markdown 到 .crabmate/exports/；F10 本机运行状况（无需启动 HTTP）。退出时保存 .crabmate/tui_session.json，启动自动加载（首条 system 随当前配置更新）。",
            ),
            Line::from(
                "【模型运行中】Ctrl+G 协作停止生成；Ctrl+Shift+G 强制中止任务（子进程工具可能仍须等待或依赖强制）。",
            ),
            Line::from("【外观】F3 代码高亮主题；F4 Markdown 暗/亮；F5 高对比度。"),
            Line::from("【工作区列表】Enter 打开/进入目录；Backspace 上级；↑↓ 选择；r 刷新。"),
            Line::from("【任务】Space 勾选/取消；↑↓；r 刷新。"),
            Line::from(
                "【日程】t 提醒子列表、e 日程子列表；Space 完成/取消提醒；a 新增提醒（打开提示行）；↑↓；r 刷新。",
            ),
            Line::from(
                "【提示行】搜索/跳转/新增提醒等弹出的一行编辑：Shift+Enter 换行，Enter 提交，Esc 取消。",
            ),
            Line::from(
                "【命令审批】←/→ 或 1/2/3；D、O、P 快捷提交；Enter 按当前选项确认。Esc 将选项切到「拒绝」，再 Enter 或 D 可确认拒绝。",
            ),
            Line::from("【文件预览】Esc 或 q 关闭。"),
            Line::raw(""),
            Line::from(
                "说明：聊天区折行算法与 Markdown 渲染区在极端长行上可能与 ratatui Wrap 略有差异，属预期范围。",
            ),
            Line::from(
                "说明：助手 Markdown 标题行首为自动大纲编号（如 1. / 1.2. / 1.2.1.），不再显示 # 符号。",
            ),
            Line::raw(""),
            Line::from("按 F1 或 Esc 关闭本帮助。"),
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

    if state.show_health {
        let w = area.width.saturating_mul(7) / 10;
        let h = area.height.saturating_mul(8) / 10;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let popup = Rect::new(x, y, w.max(40), h.max(12));

        let mut health_lines: Vec<Line<'_>> = vec![
            Line::from(Span::styled(
                "运行状况（与 GET /health 一致）",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "不发起 HTTP；按 F10 或 Esc 关闭",
                Style::default().fg(Color::DarkGray),
            )),
            Line::raw(""),
        ];
        health_lines.extend(state.health_text.lines().map(|s| Line::raw(s.to_string())));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .title(" 健康检查 ");
        let para = Paragraph::new(health_lines)
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(Clear, popup);
        f.render_widget(para, popup);
    }
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

/// 底栏 `status_line`：`模型：` 与模型名分色；`高对比度：… | 模型：…` 同理。其余整段粗体截断。
fn status_meta_spans(
    meta: &str,
    max_display_width: usize,
    high_contrast: bool,
) -> Vec<Span<'static>> {
    const MODEL_PREFIX: &str = "模型：";
    if max_display_width == 0 {
        return Vec::new();
    }

    let label_style = Style::default()
        .fg(if high_contrast {
            Color::Gray
        } else {
            Color::DarkGray
        })
        .add_modifier(Modifier::BOLD);
    let model_name_style = Style::default()
        .fg(if high_contrast {
            Color::LightYellow
        } else {
            Color::LightCyan
        })
        .add_modifier(Modifier::BOLD);
    let hc_prefix_style = Style::default()
        .fg(if high_contrast {
            Color::White
        } else {
            Color::Gray
        })
        .add_modifier(Modifier::BOLD);

    let fallback_plain = || {
        vec![Span::styled(
            truncate_display_width(meta, max_display_width),
            Style::default().add_modifier(Modifier::BOLD),
        )]
    };

    if let Some(name) = meta.strip_prefix(MODEL_PREFIX) {
        let pw = MODEL_PREFIX.width();
        if pw >= max_display_width {
            return fallback_plain();
        }
        let name_show = truncate_display_width(name, max_display_width.saturating_sub(pw));
        return vec![
            Span::styled(MODEL_PREFIX.to_string(), label_style),
            Span::styled(name_show, model_name_style),
        ];
    }

    if let Some((left, name)) = meta.split_once(" | 模型：") {
        let prefix = format!("{} | {}", left, MODEL_PREFIX);
        let pw = prefix.width();
        if pw >= max_display_width {
            return fallback_plain();
        }
        let name_show = truncate_display_width(name, max_display_width.saturating_sub(pw));
        return vec![
            Span::styled(prefix, hc_prefix_style),
            Span::styled(name_show, model_name_style),
        ];
    }

    fallback_plain()
}

/// 与 `draw_ui` 左侧聊天列宽度一致（65% 列减去左右 padding）。
pub(super) fn chat_inner_width_from_term_cols(term_cols: u16) -> usize {
    term_cols
        .saturating_mul(65)
        .saturating_div(100)
        .max(3)
        .saturating_sub(2) as usize
}

/// 拼接一行内全部 span 的文本（与搜索、折行估算一致）。
fn line_to_plain(line: &Line<'_>) -> String {
    line.spans.iter().fold(String::new(), |mut acc, s| {
        acc.push_str(s.content.as_ref());
        acc
    })
}

/// 将 `tui_markdown` 产出的行变为可存入 `Vec` 的 `Line<'static>`（保留样式）。
fn line_into_static(line: Line<'_>) -> Line<'static> {
    let style = line.style;
    let alignment = line.alignment;
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|s| Span::styled(s.content.into_owned(), s.style))
        .collect();
    let mut out = Line::from(spans);
    out.style = style;
    out.alignment = alignment;
    out
}

/// 仅用原始 `Message` 字段做指纹，缓存命中时不必再跑 LaTeX / 剥标签。
fn line_cache_fingerprint(m: &Message) -> u64 {
    let mut h = DefaultHasher::new();
    m.role.hash(&mut h);
    match &m.content {
        Some(s) => {
            1u8.hash(&mut h);
            s.hash(&mut h);
        }
        None => {
            0u8.hash(&mut h);
        }
    }
    h.finish()
}

fn message_body_for_chat_display(m: &Message) -> String {
    let raw = m.content.as_deref().unwrap_or("");
    if m.role == "assistant" {
        return assistant_markdown_source_for_display(raw);
    }
    let display_raw = if m.role == "tool" {
        tool_content_for_display(raw)
    } else {
        raw.to_string()
    };
    latex_math_to_unicode(&display_raw)
}

/// 单条消息对应的绘制行与纯文本行（不含尾部消息间空行）。
fn render_message_chat_lines(
    m: &Message,
    rendered: &str,
    state: &TuiState,
    chat_inner_width: usize,
) -> (Vec<Line<'static>>, Vec<String>) {
    let mut draw_lines: Vec<Line<'static>> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();
    let header_style = Style::default().add_modifier(Modifier::BOLD);
    let role = if m.role == "user" { "我" } else { "模型" };
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
        draw_lines.push(Line::from(Span::styled(role_padded.clone(), header_style)));
        plain_lines.push(role_padded);
    } else if m.role != "assistant" {
        let h = format!("{}:", role);
        draw_lines.push(Line::from(Span::styled(h.clone(), header_style)));
        plain_lines.push(h);
    }
    if m.role == "assistant" {
        let theme = code_themes()[state.code_theme_idx];
        let text = match (state.md_style, state.high_contrast) {
            (0, false) => {
                let options = Options::new(DarkStyleSheet)
                    .with_code_theme(theme)
                    .with_outline_heading_numbers(true);
                markdown_to_text(rendered, &options)
            }
            (0, true) => {
                let options = Options::new(HighContrastDarkStyleSheet)
                    .with_code_theme(theme)
                    .with_outline_heading_numbers(true);
                markdown_to_text(rendered, &options)
            }
            (1, false) => {
                let options = Options::new(LightStyleSheet)
                    .with_code_theme(theme)
                    .with_outline_heading_numbers(true);
                markdown_to_text(rendered, &options)
            }
            (1, true) => {
                let options = Options::new(HighContrastLightStyleSheet)
                    .with_code_theme(theme)
                    .with_outline_heading_numbers(true);
                markdown_to_text(rendered, &options)
            }
            _ => {
                let options = Options::new(DarkStyleSheet)
                    .with_code_theme(theme)
                    .with_outline_heading_numbers(true);
                markdown_to_text(rendered, &options)
            }
        };
        for tl in text.lines {
            let owned = line_into_static(tl);
            plain_lines.push(line_to_plain(&owned));
            draw_lines.push(owned);
        }
    } else {
        for l in rendered.lines() {
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
            plain_lines.push(line_str.clone());
            draw_lines.push(Line::raw(line_str));
        }
    }
    (draw_lines, plain_lines)
}

/// 分阶段规划摘要：参考 Cursor「思考」——终端无法缩小字号，用 **粗体 + DIM** 弱化层级；高对比度下略提高明度以免过淡。
fn staged_plan_think_style(state: &TuiState) -> Style {
    if state.high_contrast {
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::DarkGray)
    } else {
        Style::default().add_modifier(Modifier::BOLD | Modifier::DIM)
    }
}

/// 与 `agent_turn` 注入的分步 user（正文含 `【分步执行`）区分，取**本轮真人提问**所在下标，供主聊天区挂载 `staged_plan_log`。
/// 从后往前找：最后一条「非分步注入」的 `user` 即当前轮用户（同步全量 `messages` 后仍成立）。
fn staged_plan_anchor_after_message_index(messages: &[Message]) -> Option<usize> {
    const STAGED_STEP_USER_MARKER: &str = "【分步执行";
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role != "user" {
            continue;
        }
        let c = m.content.as_deref().unwrap_or("");
        if c.contains(STAGED_STEP_USER_MARKER) {
            continue;
        }
        return Some(i);
    }
    None
}

fn push_staged_plan_chat_block(
    draw_lines: &mut Vec<Line<'static>>,
    plain_lines: &mut Vec<String>,
    state: &TuiState,
) {
    if state.staged_plan_log.is_empty() {
        return;
    }
    let sty = staged_plan_think_style(state);
    draw_lines.push(Line::from(vec![Span::styled(
        STAGED_PLAN_SECTION_HEADER,
        sty,
    )]));
    plain_lines.push(STAGED_PLAN_SECTION_HEADER.to_string());
    for row in &state.staged_plan_log {
        let row = latex_math_to_unicode(row);
        draw_lines.push(Line::from(vec![Span::styled(row.clone(), sty)]));
        plain_lines.push(row);
    }
}

/// 与 `draw_chat` 相同的逻辑行：第一项为带样式绘制行；第二项为同序纯文本（供 Ctrl+F 匹配）；第三项为每条非 system 消息首行索引。
pub(super) fn build_chat_scroll_lines(
    state: &mut TuiState,
    chat_inner_width: usize,
) -> (Vec<Line<'static>>, Vec<String>, Vec<usize>) {
    {
        let c = &mut state.chat_line_build_cache;
        if c.chat_inner_width != chat_inner_width
            || c.md_style != state.md_style
            || c.high_contrast != state.high_contrast
            || c.code_theme_idx != state.code_theme_idx
        {
            c.per_message.clear();
            c.chat_inner_width = chat_inner_width;
            c.md_style = state.md_style;
            c.high_contrast = state.high_contrast;
            c.code_theme_idx = state.code_theme_idx;
        }
        if c.per_message.len() != state.messages.len() {
            c.per_message.resize_with(state.messages.len(), || None);
        }
    }

    let mut draw_lines: Vec<Line<'static>> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();
    let mut message_start_lines: Vec<usize> = Vec::new();

    let plan_anchor_idx = staged_plan_anchor_after_message_index(&state.messages);

    for (i, m) in state.messages.iter().enumerate() {
        if m.role == "system" {
            continue;
        }
        message_start_lines.push(draw_lines.len());

        let fp = line_cache_fingerprint(m);

        let cached = state
            .chat_line_build_cache
            .per_message
            .get(i)
            .and_then(|slot| slot.as_ref())
            .filter(|e| e.content_fingerprint == fp);

        let (mut d, mut p) = if let Some(e) = cached {
            (e.draw.clone(), e.plain.clone())
        } else {
            let rendered = message_body_for_chat_display(m);
            let (draw, plain) = render_message_chat_lines(m, &rendered, state, chat_inner_width);
            state.chat_line_build_cache.per_message[i] = Some(ChatMessageLineCacheEntry {
                content_fingerprint: fp,
                draw: draw.clone(),
                plain: plain.clone(),
            });
            (draw, plain)
        };

        draw_lines.append(&mut d);
        plain_lines.append(&mut p);
        draw_lines.push(Line::raw(""));
        plain_lines.push(String::new());

        // 分阶段规划：挂在**真人 user** 之后。若用「最后一条 user」会在同步后命中 `【分步执行` 注入行，规划块被挤到会话末尾，看起来像「没打印规划」。
        if Some(i) == plan_anchor_idx && !state.staged_plan_log.is_empty() {
            push_staged_plan_chat_block(&mut draw_lines, &mut plain_lines, state);
            draw_lines.push(Line::raw(""));
            plain_lines.push(String::new());
        }
    }

    (draw_lines, plain_lines, message_start_lines)
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

    let chat_inner_width = vchunks[0].width.saturating_sub(2) as usize;
    let (mut lines, _, _) = build_chat_scroll_lines(state, chat_inner_width);
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
    let meta = state.status_line.trim().replace(['\n', '\r'], " ");
    let status_line = if meta.is_empty() {
        Line::from(vec![
            Span::styled(" ", bold),
            Span::styled(
                phase_label,
                Style::default()
                    .fg(phase_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        // 「 ␣阶段 │ 」占宽，右侧说明单独按列宽截断，避免整串截断吃掉彩色阶段词
        let prefix_w = 1usize.saturating_add(phase_label.width()).saturating_add(3); // " │ "
        let meta_max = inner_cols.saturating_sub(prefix_w).max(1);
        let mut spans = vec![
            Span::styled(" ", bold),
            Span::styled(
                phase_label,
                Style::default()
                    .fg(phase_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │ ", bold),
        ];
        spans.extend(status_meta_spans(&meta, meta_max, state.high_contrast));
        Line::from(spans)
    };
    let status = Paragraph::new(status_line);
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
        RightTab::Queue => {
            let mut lines = Vec::new();
            lines.push(Line::raw(
                "本页：终端会话内的对话回合摘要；与浏览器 `--serve` 的 HTTP 队列相互独立。",
            ));
            lines.push(Line::raw(""));
            lines.push(Line::raw(format!(
                "配置（与 Web 共用 `[agent] chat_queue_*`）：并发上限 {}，等待槽 {}。",
                state.chat_queue_max_concurrent, state.chat_queue_max_pending
            )));
            lines.push(Line::raw(
                "`--serve` 时由 ChatJobQueue 执行多路排队；此处仅作对照。",
            ));
            lines.push(Line::raw(""));
            if let Some(jid) = state.tui_active_job_id {
                let phase = state.model_phase.label();
                lines.push(Line::raw(format!(
                    "当前回合：job_id={jid} 进行中（状态栏阶段：{phase}）"
                )));
            } else {
                lines.push(Line::raw("当前回合：空闲"));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "分阶段规划日志（本轮）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if state.staged_plan_log.is_empty() {
                lines.push(Line::raw(
                    "（未启用 staged_plan_execution 或尚无步骤通知；启用后此处显示规划摘要与各步进度）",
                ));
            } else {
                for row in &state.staged_plan_log {
                    lines.push(Line::raw(row.as_str()));
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::raw("最近本会话回合（最新在上）："));
            if state.tui_turn_recent.is_empty() {
                lines.push(Line::raw("（尚无记录）"));
            } else {
                for rec in state.tui_turn_recent.iter().rev().take(24) {
                    let status = if rec.hard_aborted {
                        "中止"
                    } else if rec.user_cancelled {
                        "已取消"
                    } else if rec.ok {
                        "成功"
                    } else {
                        "失败"
                    };
                    let mut s = format!("#{} tui {} {}ms", rec.job_id, status, rec.duration_ms);
                    if let Some(ref e) = rec.error_preview {
                        s.push_str(&format!(" — {e}"));
                    }
                    lines.push(Line::raw(s));
                }
            }
            let queue_block = Block::default()
                .borders(Borders::NONE)
                .padding(Padding::symmetric(1, 1));
            let w = Paragraph::new(lines)
                .block(queue_block)
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

#[cfg(test)]
fn test_assistant_message(content: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

#[cfg(test)]
#[test]
fn staged_plan_anchor_skips_step_injection_users() {
    let messages = vec![
        Message::system_only("s"),
        Message::user_only("写个 cpp"),
        test_assistant_message("plan"),
        Message::user_only("【分步执行 1/2】只完成本步"),
        test_assistant_message("done step"),
    ];
    assert_eq!(staged_plan_anchor_after_message_index(&messages), Some(1));
}

#[cfg(test)]
#[test]
fn staged_plan_anchor_plain_user_when_no_injection() {
    let messages = vec![
        Message::system_only("s"),
        Message::user_only("hi"),
        test_assistant_message("ok"),
    ];
    assert_eq!(staged_plan_anchor_after_message_index(&messages), Some(1));
}
