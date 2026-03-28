//! REPL 终端样式：**集中在 [`CliReplStyle`]**（配色、是否启用 ANSI）；尊重 **`NO_COLOR`**，非 TTY 时不写入转义序列。

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// 左缘空白列数（`"  "`）。
const HELP_LEFT: usize = 2;
/// 命令列与说明之间的空隙列数。
const HELP_GAP: usize = 1;
/// 表格布局时第一行至少留给说明的列数；不足则改为「命令单独一行」。
const HELP_DESC_MIN: usize = 8;

fn pad_cmd_to_display_width(cmd: &str, target: usize) -> String {
    let mut s = cmd.to_string();
    while s.width() < target {
        s.push(' ');
    }
    s
}

fn spaces_to_display_width(target: usize) -> String {
    let mut s = String::new();
    while s.width() < target {
        s.push(' ');
    }
    s
}

/// 无空格且超过 `max_w` 显示宽度的片段，按字符边界硬拆行。
fn break_long_word(word: &str, max_w: usize) -> Vec<String> {
    let max_w = max_w.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut acc = 0usize;
    for ch in word.chars() {
        let cw = ch.width().unwrap_or(0).max(1);
        if acc + cw > max_w {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                acc = 0;
            }
            if cw > max_w {
                out.push(ch.to_string());
                continue;
            }
        }
        cur.push(ch);
        acc += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 按空白分词软换行；显示宽度用 [`UnicodeWidthStr`]（CJK 等按终端惯例计宽）。
fn wrap_help_description(text: &str, max_w: usize) -> Vec<String> {
    let max_w = max_w.max(1);
    let text = text.trim();
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut cur_w = 0usize;

    for word in text.split_whitespace() {
        let ww = word.width();
        let need_space = !current.is_empty();
        let sep_w = need_space as usize;

        if ww > max_w {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                cur_w = 0;
            }
            for chunk in break_long_word(word, max_w) {
                lines.push(chunk);
            }
            continue;
        }

        if cur_w + sep_w + ww > max_w {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            current.push_str(word);
            cur_w = ww;
        } else {
            if need_space {
                current.push(' ');
                cur_w += 1;
            }
            current.push_str(word);
            cur_w += ww;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// CLI REPL 的终端样式：构造时固定 stdout/stderr 是否着色，所有横幅、帮助、成功/错误行均经此结构输出。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CliReplStyle {
    use_color_stdout: bool,
    use_color_stderr: bool,
}

impl CliReplStyle {
    // --- 暗色终端友好 RGB 主题（仅此 impl 块内调整即可统一改 REPL 观感）---
    const C_MUTED: Color = Color::Rgb {
        r: 100,
        g: 108,
        b: 118,
    };
    const C_BANNER_FRAME: Color = Color::Rgb {
        r: 72,
        g: 82,
        b: 96,
    };
    const C_BANNER_TITLE: Color = Color::Rgb {
        r: 78,
        g: 201,
        b: 214,
    };
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
    const C_HELP_TITLE: Color = Color::Rgb {
        r: 250,
        g: 195,
        b: 92,
    };
    const C_HELP_CMD: Color = Color::Rgb {
        r: 130,
        g: 214,
        b: 165,
    };
    const C_HELP_DESC: Color = Color::Rgb {
        r: 118,
        g: 124,
        b: 138,
    };

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
        let mut out = io::stdout();
        writeln!(out, "{msg}")?;
        out.flush()
    }

    pub(crate) fn print_success(&self, msg: &str) -> io::Result<()> {
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_SUCCESS),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "{msg}")?;
        self.queue_reset(&mut out, true)?;
        out.flush()
    }

    pub(crate) fn eprint_error(&self, msg: &str) -> io::Result<()> {
        let mut err = io::stderr();
        if self.use_color_stderr {
            queue!(
                err,
                SetForegroundColor(Self::C_ERROR),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(err, "{msg}")?;
        self.queue_reset(&mut err, false)?;
        err.flush()
    }

    /// `/help`：节标题 + 命令/说明列（宽度随终端、`unicode-width` 软换行）。
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

        let (tw, _) = crossterm::terminal::size().unwrap_or((80, 24));
        let inner = tw as usize;
        let max_cmd_w = rows.iter().map(|(c, _)| c.width()).max().unwrap_or(0);
        let table_ok = inner >= HELP_LEFT + max_cmd_w + HELP_GAP + HELP_DESC_MIN;
        let w_desc_table = inner
            .saturating_sub(HELP_LEFT + max_cmd_w + HELP_GAP)
            .max(1);
        let w_desc_stacked = inner.saturating_sub(HELP_LEFT).max(1);
        let cont_pad = spaces_to_display_width(max_cmd_w + HELP_GAP);

        for (cmd, desc) in rows {
            let desc_lines = if table_ok {
                wrap_help_description(desc, w_desc_table)
            } else {
                wrap_help_description(desc, w_desc_stacked)
            };

            if !table_ok {
                if self.use_color_stdout {
                    queue!(
                        out,
                        SetForegroundColor(Self::C_HELP_CMD),
                        SetAttribute(Attribute::Bold)
                    )?;
                }
                writeln!(out, "  {cmd}")?;
                self.queue_reset(&mut out, true)?;
                for line in &desc_lines {
                    if self.use_color_stdout {
                        queue!(
                            out,
                            SetForegroundColor(Self::C_HELP_DESC),
                            SetAttribute(Attribute::Dim)
                        )?;
                    }
                    writeln!(out, "  {line}")?;
                    self.queue_reset(&mut out, true)?;
                }
                continue;
            }

            let padded = pad_cmd_to_display_width(cmd, max_cmd_w);

            for (i, line) in desc_lines.iter().enumerate() {
                if self.use_color_stdout {
                    queue!(
                        out,
                        SetForegroundColor(Self::C_HELP_CMD),
                        SetAttribute(Attribute::Bold)
                    )?;
                }
                if i == 0 {
                    write!(out, "  {padded} ")?;
                } else {
                    write!(out, "  {cont_pad}")?;
                }
                self.queue_reset(&mut out, true)?;
                if self.use_color_stdout {
                    queue!(
                        out,
                        SetForegroundColor(Self::C_HELP_DESC),
                        SetAttribute(Attribute::Dim)
                    )?;
                }
                writeln!(out, "{line}")?;
                self.queue_reset(&mut out, true)?;
            }
        }
        writeln!(out)?;
        self.writeln_muted_line("退出：quit · exit · Ctrl+D")?;
        Ok(())
    }
}
