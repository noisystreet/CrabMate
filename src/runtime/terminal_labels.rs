//! CLI/serve 终端回显：「Agent」前缀着色，以及 **`plain_terminal_stream`** 下
//! `reasoning_content`（偏亮冷灰）与 `content`（默认前景）的分色（尊重 **`NO_COLOR`**、非 TTY 不着色）。

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

/// 助手回复前缀：`Agent: `，加粗 + 洋红。
pub(crate) fn write_agent_message_prefix<W: Write + QueueableCommand>(w: &mut W) -> io::Result<()> {
    queue!(
        w,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Magenta)
    )?;
    write!(w, "Agent: ")?;
    queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    Ok(())
}

/// 未设 **`NO_COLOR`** 且 stdout 为 TTY 时允许为助手正文写 ANSI。
#[inline]
pub(crate) fn stdout_use_cli_ansi_color() -> bool {
    crate::runtime::terminal_ansi::terminal_stdout_use_color()
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
