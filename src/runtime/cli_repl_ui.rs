//! REPL 终端样式：**集中在 [`CliReplStyle`]**（配色、是否启用 ANSI）；尊重 **`NO_COLOR`**，非 TTY 时不写入转义序列。

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};

/// CLI REPL 的终端样式：构造时固定 stdout/stderr 是否着色，所有横幅、帮助、成功/错误行均经此结构输出。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CliReplStyle {
    use_color_stdout: bool,
    use_color_stderr: bool,
}

impl CliReplStyle {
    // --- 配色（仅此 impl 块内调整即可统一改 REPL 观感）---
    const C_MUTED: Color = Color::DarkGrey;
    const C_BANNER_FRAME: Color = Color::DarkGrey;
    const C_BANNER_TITLE: Color = Color::Cyan;
    const C_SUCCESS: Color = Color::Green;
    const C_ERROR: Color = Color::Red;
    const C_HELP_TITLE: Color = Color::Yellow;
    const C_HELP_CMD: Color = Color::Green;
    const C_HELP_DESC: Color = Color::DarkGrey;

    pub(crate) fn new() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        Self {
            use_color_stdout: !no_color && io::stdout().is_terminal(),
            use_color_stderr: !no_color && io::stderr().is_terminal(),
        }
    }

    fn queue_reset(
        &self,
        out: &mut (impl Write + QueueableCommand),
        stdout: bool,
    ) -> io::Result<()> {
        if (stdout && self.use_color_stdout) || (!stdout && self.use_color_stderr) {
            queue!(out, SetAttribute(Attribute::Reset), ResetColor)?;
        }
        Ok(())
    }

    fn writeln_muted_line(&self, line: &str) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(out, SetForegroundColor(Self::C_MUTED))?;
        }
        writeln!(out, "{line}")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    /// 启动横幅：模型、工作区、工具数与简要说明。
    pub(crate) fn print_banner(
        &self,
        model: &str,
        work_dir: &Path,
        tool_count: usize,
    ) -> io::Result<()> {
        let mut out = io::stdout();
        let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
        let inner = (tw as usize).saturating_sub(6).clamp(28, 72);
        let bar = "─".repeat(inner);

        writeln!(out)?;
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_BANNER_FRAME),
                SetAttribute(Attribute::Dim)
            )?;
        }
        writeln!(out, "  ╭{bar}╮")?;
        self.queue_reset(&mut out, true)?;

        let mid_w = inner.saturating_sub(2).max(12);
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_BANNER_TITLE),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "  │ {:^width$} │", "CrabMate · REPL", width = mid_w)?;
        self.queue_reset(&mut out, true)?;

        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_BANNER_FRAME),
                SetAttribute(Attribute::Dim)
            )?;
        }
        writeln!(out, "  ╰{bar}╯")?;
        self.queue_reset(&mut out, true)?;

        self.writeln_muted_line(&format!(
            "  模型 {}  ·  工作区 {}",
            model,
            work_dir.display()
        ))?;
        let tools_line = if tool_count == 0 {
            "  工具 已关闭（--no-tools）".to_string()
        } else {
            format!("  工具 {tool_count} 个可用")
        };
        self.writeln_muted_line(&tools_line)?;
        self.writeln_muted_line(
            "  输入消息对话；/help 内建命令 · 行首 `$` 进入本地 shell（提示变为 bash#:，`$` 不回显）· quit / exit / Ctrl+D 退出",
        )?;
        self.writeln_muted_line("  非白名单 run_command 将询问：y 一次 / a 本会话允许该命令名")?;
        writeln!(out)?;
        out.flush()
    }

    pub(crate) fn print_farewell(&self) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(out, SetForegroundColor(Self::C_MUTED))?;
        }
        writeln!(out, "再见。")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    pub(crate) fn print_line(&self, msg: &str) -> io::Result<()> {
        let mut out = io::stdout();
        writeln!(out, "{msg}")?;
        out.flush()
    }

    pub(crate) fn print_success(&self, msg: &str) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(out, SetForegroundColor(Self::C_SUCCESS))?;
        }
        writeln!(out, "{msg}")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    pub(crate) fn eprint_error(&self, msg: &str) -> io::Result<()> {
        let mut err = io::stderr();
        if self.use_color_stderr {
            queue!(err, SetForegroundColor(Self::C_ERROR))?;
        }
        writeln!(err, "{msg}")?;
        self.queue_reset(&mut err, false)?;
        err.flush()
    }

    /// `/help`：节标题 + 命令/说明列。
    pub(crate) fn print_help(&self) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_HELP_TITLE),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "内建命令（不会发给模型）")?;
        self.queue_reset(&mut out, true)?;

        let rows: &[(&str, &str)] = &[
            ("/clear", "清空对话，仅保留当前 system 提示词"),
            ("/model", "显示 model、api_base、temperature、llm_seed"),
            ("/workspace", "显示当前工作区"),
            (
                "/workspace <路径>",
                "切换工作区（须为已存在目录，别名 /cd）",
            ),
            ("/tools", "列出当前加载的工具名"),
            ("/help, /?", "本说明"),
            (
                "$ → bash#:",
                "交互终端行首按 `$` 后提示变为 bash#: 并输入命令；管道输入仍可用 `$ <命令>`",
            ),
        ];
        for (cmd, desc) in rows {
            if self.use_color_stdout {
                queue!(
                    out,
                    SetForegroundColor(Self::C_HELP_CMD),
                    SetAttribute(Attribute::Bold)
                )?;
            }
            write!(out, "  {cmd:<26}")?;
            self.queue_reset(&mut out, true)?;
            if self.use_color_stdout {
                queue!(out, SetForegroundColor(Self::C_HELP_DESC))?;
            }
            writeln!(out, "{desc}")?;
            self.queue_reset(&mut out, true)?;
        }
        writeln!(out)?;
        self.writeln_muted_line("退出：quit · exit · Ctrl+D")?;
        Ok(())
    }
}
