//! CLI 无 SSE 通道（`out: None`）时，将分阶段规划与工具结果写到 stdout，与 TUI 聊天区信息对齐。

use log::debug;

use crate::redact;

use crossterm::{
    queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

use super::latex_unicode::latex_math_to_unicode;
use super::message_display::{tool_content_for_display_full, user_message_for_chat_display};
use super::plan_section::STAGED_PLAN_SECTION_HEADER;

/// 与 TUI 聊天区展示一致：正文经 **`user_message_for_chat_display`**（含分步注入 user 长句压缩、LaTeX），再按行打印到 stdout。
pub(crate) fn print_staged_plan_notice(clear_before: bool, text: &str) -> io::Result<()> {
    let display = user_message_for_chat_display(text);
    debug!(
        target: "crabmate::print",
        "CLI 打印分阶段规划通知 clear_before={} text_len={} text_preview={}",
        clear_before,
        display.len(),
        redact::preview_chars(&display, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
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
    for line in display.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        queue!(w, SetForegroundColor(Color::DarkGrey))?;
        writeln!(w, "{t}")?;
        queue!(w, ResetColor)?;
    }
    w.flush()
}

/// 工具执行结束后打印名称与正文（正文用完整格式化，与聊天区「可省略实际输出」策略独立），过长按 `max_chars` 截断（近似字符数）。
pub(crate) fn print_tool_result_terminal(
    name: &str,
    raw_result: &str,
    max_chars: usize,
) -> io::Result<()> {
    debug!(
        target: "crabmate::print",
        "CLI 打印工具结果 tool={} raw_len={} raw_preview={}",
        name,
        raw_result.len(),
        redact::preview_chars(raw_result, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let mut body = tool_content_for_display_full(raw_result);
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
    debug!(
        target: "crabmate::print",
        "CLI 工具结果展示正文 body_len={} body_preview={}",
        body.len(),
        redact::preview_chars(&body, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    w.flush()
}
