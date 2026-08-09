//! REPL 终端样式：**集中在 [`CliReplStyle`]**（配色、是否启用 ANSI）；尊重 **`NO_COLOR`**，非 TTY 时不写入转义序列。
//!
//! **捕获模式**：[`CliReplStyle::new_tui_capture`] 将本应写入终端的行追加到缓冲区（纯文本、无 ANSI），供全屏 TUI 写入 transcript。

mod banner;
mod banner_highlights_section;
mod config_summary;
mod config_summary_render;
mod help;
mod tables;

use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
// --- 与横幅 / `/help` 节标题共用的 RGB（单一真源）；`terminal_labels` 输入提示与此对齐 ---
const RGB_BANNER_TITLE: Color = Color::Rgb {
    r: 78,
    g: 201,
    b: 214,
};
const RGB_HELP_TITLE: Color = Color::Rgb {
    r: 250,
    g: 195,
    b: 92,
};

/// `/help` 节标题、分阶段 CLI 转录首行等**节级**前缀色（琥珀）；与 `bash#:` 提示同色。
pub(crate) const CLI_REPL_HELP_TITLE_FG: Color = RGB_HELP_TITLE;
/// `/help` 命令列与 **`### 工具 · …`** 等**强调前缀**色（青绿）。
pub(crate) const CLI_REPL_HELP_CMD_FG: Color = Color::Rgb {
    r: 130,
    g: 214,
    b: 165,
};
/// `/help` 说明列与 CLI 转录**次要正文**色（冷灰）。
pub(crate) const CLI_REPL_HELP_DESC_FG: Color = Color::Rgb {
    r: 118,
    g: 124,
    b: 138,
};

/// 与 REPL 横幅、`terminal_cli_transcript` 一致：**未**设 **`NO_COLOR`** 且 **stdout** 为 TTY 时写入 ANSI。
pub(crate) fn cli_repl_stdout_use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}

/// **`NO_COLOR`** 未设置且 **stderr** 为 TTY 时写入 ANSI（与 [`CliReplStyle::eprint_error`] 等一致）。
pub(crate) fn cli_repl_stderr_use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && io::stderr().is_terminal()
}

/// 「我:」「bash#:」与可编辑输入之间的分隔（单字宽 `▸`，两侧空格便于扫读）。
pub(crate) const CLI_PROMPT_AFTER_COLON: &str = " ▸ ";
/// 用户输入行提示前景色（同 [`CliReplStyle`] 横幅标题色）。
pub(crate) const CLI_PROMPT_USER_FG: Color = RGB_BANNER_TITLE;
/// `bash#:` 提示前景色（同 [`CLI_REPL_HELP_TITLE_FG`]）。
pub(crate) const CLI_PROMPT_BASH_FG: Color = CLI_REPL_HELP_TITLE_FG;

/// REPL 顶栏 FIGlet 风格 **CrabMate**（固定 6 行 ASCII；`r"..."` 保留 `\`）。
const BANNER_CRABMATE_ART: &[&str] = &[
    r"  ______ .______          ___      .______   .___  ___.      ___   .___________. _______ ",
    r" /      ||   _  \        /   \     |   _  \  |   \/   |     /   \  |           ||   ____|",
    r"|  ,----'|  |_)  |      /  ^  \    |  |_)  | |  \  /  |    /  ^  \ `---|  |----`|  |__   ",
    r"|  |     |      /      /  /_\  \   |   _  <  |  |\/|  |   /  /_\  \    |  |     |   __|  ",
    r"|  `----.|  |\  \----./  _____  \  |  |_)  | |  |  |  |  /  _____  \   |  |     |  |____ ",
    r" \______|| _| `._____/__/     \__\ |______/  |__|  |__| /__/     \__\  |__|     |_______|",
];

/// CLI REPL 的终端样式：构造时固定 stdout/stderr 是否着色，所有横幅、帮助、成功/错误行均经此结构输出。
#[derive(Debug, Clone)]
pub(crate) struct CliReplStyle {
    use_color_stdout: bool,
    use_color_stderr: bool,
    capture: Option<Arc<Mutex<Vec<String>>>>,
}

impl CliReplStyle {
    // --- 暗色终端友好 RGB 主题（仅此 impl 块内调整即可统一改 REPL 观感）---
    const C_MUTED: Color = Color::Rgb {
        r: 100,
        g: 108,
        b: 118,
    };
    const C_BANNER_TITLE: Color = RGB_BANNER_TITLE;
    const C_SUCCESS: Color = Color::Rgb {
        r: 102,
        g: 217,
        b: 145,
    };
    const C_ERROR: Color = Color::Rgb {
        r: 255,
        g: 118,
        b: 118,
    };
    const C_HELP_TITLE: Color = CLI_REPL_HELP_TITLE_FG;
    const C_HELP_CMD: Color = CLI_REPL_HELP_CMD_FG;
    const C_HELP_DESC: Color = CLI_REPL_HELP_DESC_FG;

    pub(crate) fn new() -> Self {
        Self {
            use_color_stdout: cli_repl_stdout_use_color(),
            use_color_stderr: cli_repl_stderr_use_color(),
            capture: None,
        }
    }

    /// 全屏 TUI：`print_*` / `eprint_*` 写入的行追加到 `buf`（无 ANSI；成功 `[ok]`、错误 `[err]`）。
    pub(crate) fn new_tui_capture(buf: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            use_color_stdout: false,
            use_color_stderr: false,
            capture: Some(buf),
        }
    }

    fn push_capture(&self, line: String) -> bool {
        let Some(cap) = self.capture.as_ref() else {
            return false;
        };
        cap.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        true
    }

    pub(super) fn queue_reset(
        &self,
        out: &mut (impl Write + QueueableCommand),
        stdout: bool,
    ) -> io::Result<()> {
        if (stdout && self.use_color_stdout) || (!stdout && self.use_color_stderr) {
            queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;
        }
        Ok(())
    }

    pub(super) fn writeln_muted_line(&self, line: &str) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_MUTED),
                SetAttribute(Attribute::Dim)
            )?;
        }
        writeln!(out, "{line}")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }
    pub(crate) fn print_farewell(&self) -> io::Result<()> {
        if self.capture.is_some() {
            return Ok(());
        }
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_MUTED),
                SetAttribute(Attribute::Dim)
            )?;
        }
        writeln!(out, "再见。")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    pub(crate) fn print_line(&self, msg: &str) -> io::Result<()> {
        if self.push_capture(msg.to_string()) {
            return Ok(());
        }
        let mut out = io::stdout();
        writeln!(out, "{msg}")?;
        out.flush()
    }

    /// 成功反馈行：着色 TTY 下前缀 **`✓`**；**`NO_COLOR`** 或非 TTY 下为 **`[ok]`**，避免缺字字体显示为乱码。
    pub(crate) fn print_success(&self, msg: &str) -> io::Result<()> {
        if self.push_capture(format!("[ok] {msg}")) {
            return Ok(());
        }
        let mut out = io::stdout();
        let prefix = if self.use_color_stdout {
            "✓ "
        } else {
            "[ok] "
        };
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_SUCCESS),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "{prefix}{msg}")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    /// 错误行：着色 TTY 下前缀 **`✗`**；**`NO_COLOR`** 或非 TTY 下为 **`[err]`**。
    pub(crate) fn eprint_error(&self, msg: &str) -> io::Result<()> {
        if self.push_capture(format!("[err] {msg}")) {
            return Ok(());
        }
        let mut err = io::stderr();
        let prefix = if self.use_color_stderr {
            "✗ "
        } else {
            "[err] "
        };
        if self.use_color_stderr {
            queue!(
                err,
                SetForegroundColor(Self::C_ERROR),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(err, "{prefix}{msg}")?;
        self.queue_reset(&mut err, false)?;
        err.flush()
    }
}
