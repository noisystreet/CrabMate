//! CLI 对话里「我」「Agent」前缀的着色与加粗（`runtime::cli` 与 `llm::api` 共用）。

use log::debug;

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

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
