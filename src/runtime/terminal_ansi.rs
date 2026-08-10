//! 终端 stdout 着色约定（原 `cli_repl_ui` 主题色；供 `terminal_labels` / `terminal_cli_transcript`）。
//! 尊重 **`NO_COLOR`**；非 TTY 时不写入转义序列。

use std::io::{self, IsTerminal};

use crossterm::style::Color;

const RGB_HELP_TITLE: Color = Color::Rgb {
    r: 250,
    g: 195,
    b: 92,
};

/// 转录/节级前缀色（琥珀）。
pub(crate) const TERMINAL_HELP_TITLE_FG: Color = RGB_HELP_TITLE;
/// 工具名等强调前缀色（青绿）。
pub(crate) const TERMINAL_HELP_CMD_FG: Color = Color::Rgb {
    r: 130,
    g: 214,
    b: 165,
};
/// 次要正文色（冷灰）。
pub(crate) const TERMINAL_HELP_DESC_FG: Color = Color::Rgb {
    r: 118,
    g: 124,
    b: 138,
};

/// **未**设 **`NO_COLOR`** 且 **stdout** 为 TTY 时写入 ANSI。
pub(crate) fn terminal_stdout_use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}
