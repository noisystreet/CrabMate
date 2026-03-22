//! CLI 无 SSE 通道（`out: None`）时，将分阶段规划与工具结果写到 stdout，与 TUI 聊天区信息对齐。

use crossterm::{
    queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

use super::latex_unicode::latex_math_to_unicode;
use super::message_display::tool_content_for_display;
use super::plan_section::STAGED_PLAN_SECTION_HEADER;

/// 与 TUI 主聊天区规划块同一节标题（`plan_section::STAGED_PLAN_SECTION_HEADER`）；`clear_before` 时先打该标题再打印通知正文。
pub(crate) fn print_staged_plan_notice(clear_before: bool, text: &str) -> io::Result<()> {
    let mut w = io::stdout();
    if clear_before {
        queue!(
            w,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::DarkYellow)
        )?;
        writeln!(w, "\n{STAGED_PLAN_SECTION_HEADER}")?;
        queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    }
    for line in text.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        let shown = latex_math_to_unicode(t);
        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        writeln!(w, "{shown}")?;
        queue!(w, ResetColor)?;
    }
    w.flush()
}

/// 工具执行结束后打印名称与正文（正文与 TUI 一致取 `human_summary`），过长按 `max_chars` 截断（近似字符数）。
pub(crate) fn print_tool_result_terminal(
    name: &str,
    raw_result: &str,
    max_chars: usize,
) -> io::Result<()> {
    let mut body = tool_content_for_display(raw_result);
    body = latex_math_to_unicode(&body);
    let n = body.chars().count();
    if n > max_chars {
        let take: String = body.chars().take(max_chars.saturating_sub(1)).collect();
        body = format!("{take}…\n[输出已截断，原约 {n} 字；完整内容已写入对话历史]");
    }
    let mut w = io::stdout();
    queue!(
        w,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Cyan)
    )?;
    writeln!(w, "\n【工具】{name}")?;
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    queue!(w, SetForegroundColor(Color::DarkGrey))?;
    writeln!(w, "{body}")?;
    queue!(w, ResetColor)?;
    w.flush()
}
