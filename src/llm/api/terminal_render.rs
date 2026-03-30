//! CLI 侧助手输出：纯文本流式分片、整段 Markdown→ANSI（与 Web 展示管线对齐的 LaTeX 等预处理）。

use std::io::{self, Write};

use log::debug;
use markdown_to_ansi::{Options, render};

use crate::redact::{self, MESSAGE_LOG_PREVIEW_CHARS};
use crate::runtime::message_display::assistant_markdown_source_for_display;
use crate::types::Message;

/// 尝试获取终端宽度；获取失败时返回 None
fn terminal_width() -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(cols, _rows)| cols as usize)
        .filter(|w| *w > 0)
}

/// 按终端显示宽度估算行数（宽字符如中文按 2 列计）；仅单测使用——流式结束不再依赖行数做光标回退。
#[cfg(test)]
fn count_display_lines(content: &str, term_width: usize) -> usize {
    use unicode_width::UnicodeWidthStr;
    let w = term_width.max(1);
    content
        .split('\n')
        .map(|line| {
            let cols = line.width().max(1);
            cols.div_ceil(w)
        })
        .sum()
}

/// CLI（`render_to_terminal && out.is_none()`）：首个非空 delta 前打 `Agent:` 前缀；**`reasoning_content`** 与 **`content`** 分色（见 [`crate::runtime::terminal_labels`]）；从思考切到终答前多写一个换行（含 **`NO_COLOR`** 时仅换行、不着色）。
pub(super) fn cli_terminal_write_plain_fragment(
    fragment: &str,
    prefix_emitted: &mut bool,
    is_reasoning: bool,
    reasoning_style_active: &mut bool,
) -> io::Result<()> {
    if fragment.is_empty() {
        return Ok(());
    }
    let use_color = crate::runtime::terminal_labels::stdout_use_cli_ansi_color();
    let mut stdout = io::stdout().lock();
    if !*prefix_emitted {
        crate::runtime::cli_wait_spinner::finish_cli_wait_spinner();
        crate::runtime::terminal_labels::write_agent_message_prefix(&mut stdout)?;
        *prefix_emitted = true;
    }
    if is_reasoning && !*reasoning_style_active {
        if use_color {
            crate::runtime::terminal_labels::queue_cli_reasoning_body_style(&mut stdout)?;
        }
        *reasoning_style_active = true;
    } else if !is_reasoning && *reasoning_style_active {
        if use_color {
            crate::runtime::terminal_labels::queue_cli_plain_body_reset(&mut stdout)?;
        }
        stdout.write_all(b"\n")?;
        *reasoning_style_active = false;
    }
    stdout.write_all(fragment.as_bytes())?;
    stdout.flush()
}

/// CLI：加粗着色 `Agent: ` + 助手展示管线（剥标签、规划可读化、LaTeX）+ `markdown_to_ansi`。
/// 仅在「非 CLI 终端」或「流式但无任何正文 delta」（如仅有 tool_calls）时使用。
pub fn terminal_render_agent_markdown(content_acc: &str) -> io::Result<()> {
    debug!(
        target: "crabmate::print",
        "CLI 终端渲染助手 Markdown content_len={} content_preview={}",
        content_acc.len(),
        redact::preview_chars(content_acc, MESSAGE_LOG_PREVIEW_CHARS)
    );
    let term_w = terminal_width().unwrap_or(80);
    let mut stdout = io::stdout();
    crate::runtime::terminal_labels::write_agent_message_prefix(&mut stdout)?;
    stdout.flush()?;
    let opts = Options {
        syntax_highlight: true,
        width: Some(term_w),
        code_bg: true,
    };
    let content = assistant_markdown_source_for_display(content_acc);
    let rendered = render(&content, &opts);
    write!(stdout, "{}", rendered)?;
    if !rendered.ends_with('\n') {
        writeln!(stdout)?;
    }
    stdout.flush()
}

/// 非流式：在已合并 `reasoning_details` 的 `msg` 上输出 CLI（纯文本分色或 Markdown）。
pub(super) fn render_non_stream_assistant_terminal(
    msg: &Message,
    plain_terminal_stream: bool,
    out_is_none: bool,
) -> io::Result<()> {
    if plain_terminal_stream && out_is_none {
        let mut prefix_emitted = false;
        let mut reasoning_style_active = false;
        if let Some(r) = msg.reasoning_content.as_deref().filter(|s| !s.is_empty()) {
            cli_terminal_write_plain_fragment(
                r,
                &mut prefix_emitted,
                true,
                &mut reasoning_style_active,
            )?;
        }
        if let Some(c) = msg.content.as_deref().filter(|s| !s.is_empty()) {
            cli_terminal_write_plain_fragment(
                c,
                &mut prefix_emitted,
                false,
                &mut reasoning_style_active,
            )?;
        }
        if prefix_emitted {
            let mut lock = io::stdout().lock();
            if reasoning_style_active {
                crate::runtime::terminal_labels::queue_cli_plain_body_reset(&mut lock)?;
            }
            let ends_nl = msg
                .content
                .as_deref()
                .map(|s| s.ends_with('\n'))
                .unwrap_or(false)
                || msg
                    .reasoning_content
                    .as_deref()
                    .map(|s| s.ends_with('\n'))
                    .unwrap_or(false);
            if !ends_nl {
                writeln!(lock)?;
            }
            lock.flush()?;
        }
    } else {
        let md = crate::runtime::message_display::assistant_raw_markdown_body_for_message(msg);
        if !md.is_empty() {
            terminal_render_agent_markdown(&md)?;
        }
    }
    Ok(())
}

/// 流式纯文本：ingest 已写完正文，此处复位样式并补末尾换行。
pub(super) fn finalize_stream_plain_terminal_suffix(
    cli_plain_reasoning_style_active: bool,
    cli_plain_prefix_emitted: bool,
    content_acc: &str,
    reasoning_acc: &str,
) -> io::Result<()> {
    let mut lock = io::stdout().lock();
    if cli_plain_reasoning_style_active {
        crate::runtime::terminal_labels::queue_cli_plain_body_reset(&mut lock)?;
    }
    let ends_nl = if !content_acc.is_empty() {
        content_acc.ends_with('\n')
    } else {
        reasoning_acc.ends_with('\n')
    };
    if cli_plain_prefix_emitted && !ends_nl {
        writeln!(lock)?;
    }
    lock.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_display_lines() {
        assert_eq!(count_display_lines("a", 80), 1);
        assert_eq!(count_display_lines("a\nb", 80), 2);
        assert_eq!(count_display_lines(&"x".repeat(80), 80), 1);
        assert_eq!(count_display_lines(&"x".repeat(81), 80), 2);
        assert_eq!(count_display_lines("中", 10), 1);
        assert_eq!(count_display_lines("中文中文中文", 10), 2);
    }
}
