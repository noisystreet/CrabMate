//! CLI 对话里「我」「Agent」前缀的着色与加粗（`runtime::cli` 与 `llm::api` 共用）；
//! 以及 **`plain_terminal_stream`** 下助手正文里 **`reasoning_content`**（偏亮冷灰）与 **`content`**（默认前景）的分色（尊重 **`NO_COLOR`**、非 TTY 不着色）。

use log::debug;

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, IsTerminal, Write};

/// 用户输入提示：`我: `，加粗 + 青色。
pub(crate) fn write_user_message_prefix<W: Write + QueueableCommand>(w: &mut W) -> io::Result<()> {
    debug!(target: "crabmate::print", "CLI 写出用户输入提示前缀（我:）");
    queue!(
        w,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Cyan)
    )?;
    write!(w, "我: ")?;
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    Ok(())
}

/// REPL 本地 shell 一行模式下的输入提示：`bash#: `，加粗 + 黄色（与「我:」区分）。
pub(crate) fn write_repl_bash_prompt_prefix<W: Write + QueueableCommand>(
    w: &mut W,
) -> io::Result<()> {
    debug!(target: "crabmate::print", "CLI 写出 REPL shell 提示前缀（bash#:）");
    queue!(
        w,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Yellow)
    )?;
    write!(w, "bash#: ")?;
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    Ok(())
}

/// 助手回复前缀：`Agent: `，加粗 + 洋红。
pub(crate) fn write_agent_message_prefix<W: Write + QueueableCommand>(w: &mut W) -> io::Result<()> {
    // 助手正文打印前的 `Agent:` 前缀；完整正文见 `llm::api::terminal_render_agent_markdown` 的 debug。
    queue!(
        w,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Magenta)
    )?;
    write!(w, "Agent: ")?;
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    Ok(())
}

/// 与 REPL 等一致：未设 **`NO_COLOR`** 且 stdout 为 TTY 时允许为助手正文写 ANSI。
#[inline]
pub(crate) fn stdout_use_cli_ansi_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

/// CLI 流式/纯文本：`reasoning_content` 片段用偏亮的冷灰（无 Dim），与 **`content`** 默认前景区分且深色终端上可读。
#[inline]
pub(crate) fn queue_cli_reasoning_body_style<W: Write + QueueableCommand>(
    w: &mut W,
) -> io::Result<()> {
    queue!(
        w,
        SetForegroundColor(Color::Rgb {
            r: 168,
            g: 182,
            b: 198,
        })
    )?;
    Ok(())
}

/// 结束「思考」样式，回到终端默认前景（供 `content`、换行等）。
#[inline]
pub(crate) fn queue_cli_plain_body_reset<W: Write + QueueableCommand>(w: &mut W) -> io::Result<()> {
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    Ok(())
}
