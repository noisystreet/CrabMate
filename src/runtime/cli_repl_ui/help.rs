//! REPL `/help` 表格布局与终端渲染。

use std::io::{self, Write};

use crossterm::queue;
use crossterm::style::{Attribute, SetAttribute, SetForegroundColor};
use unicode_width::UnicodeWidthStr;

use super::CliReplStyle;
use super::tables::{
    HELP_DESC_MIN, HELP_GAP, HELP_LEFT, REPL_HELP_ROWS, pad_cmd_to_display_width,
    spaces_to_display_width, wrap_help_description,
};

struct HelpTableLayout {
    table_ok: bool,
    max_cmd_w: usize,
    w_desc_table: usize,
    w_desc_stacked: usize,
    cont_pad: String,
}

fn compute_help_table_layout() -> HelpTableLayout {
    let rows = REPL_HELP_ROWS;
    let (tw, _) = crossterm::terminal::size().unwrap_or((80, 24));
    let inner = tw as usize;
    let max_cmd_w = rows.iter().map(|(c, _)| c.width()).max().unwrap_or(0);
    let table_ok = inner >= HELP_LEFT + max_cmd_w + HELP_GAP + HELP_DESC_MIN;
    let w_desc_table = inner
        .saturating_sub(HELP_LEFT + max_cmd_w + HELP_GAP)
        .max(1);
    let w_desc_stacked = inner.saturating_sub(HELP_LEFT).max(1);
    HelpTableLayout {
        table_ok,
        max_cmd_w,
        w_desc_table,
        w_desc_stacked,
        cont_pad: spaces_to_display_width(max_cmd_w + HELP_GAP),
    }
}

fn help_footer_lines() -> [&'static str; 2] {
    [
        "  「我:」下光标前为 /… 时按 Tab 可补全内建命令与 /export、/save-session、/mcp 子命令；bash#: 下不补全",
        "  退出：quit · exit · Ctrl+D",
    ]
}

fn render_help_body_lines(layout: &HelpTableLayout) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(String::new());
    lines.push("  内建命令".to_string());
    for (cmd, desc) in REPL_HELP_ROWS {
        let desc_lines = if layout.table_ok {
            wrap_help_description(desc, layout.w_desc_table)
        } else {
            wrap_help_description(desc, layout.w_desc_stacked)
        };
        if !layout.table_ok {
            lines.push(format!("  {cmd}"));
            for d in &desc_lines {
                lines.push(format!("  {d}"));
            }
            continue;
        }
        let padded = pad_cmd_to_display_width(cmd, layout.max_cmd_w);
        for (i, line) in desc_lines.iter().enumerate() {
            if i == 0 {
                lines.push(format!("  {padded} {line}"));
            } else {
                lines.push(format!("  {}{line}", layout.cont_pad));
            }
        }
    }
    lines.push(String::new());
    for line in help_footer_lines() {
        lines.push(line.to_string());
    }
    lines
}

impl CliReplStyle {
    fn write_help_row_stacked(
        &self,
        out: &mut io::Stdout,
        cmd: &str,
        desc_lines: &[String],
    ) -> io::Result<()> {
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_HELP_CMD),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "  {cmd}")?;
        self.queue_reset(out, true)?;
        for line in desc_lines {
            if self.use_color_stdout {
                queue!(
                    out,
                    SetForegroundColor(Self::C_HELP_DESC),
                    SetAttribute(Attribute::Dim)
                )?;
            }
            writeln!(out, "  {line}")?;
            self.queue_reset(out, true)?;
        }
        Ok(())
    }

    fn write_help_row_table_colored_line(
        &self,
        out: &mut io::Stdout,
        padded_cmd: &str,
        cont_pad: &str,
        line: &str,
        first_line: bool,
    ) -> io::Result<()> {
        queue!(
            out,
            SetForegroundColor(Self::C_HELP_CMD),
            SetAttribute(Attribute::Bold)
        )?;
        if first_line {
            write!(out, "  {padded_cmd} ")?;
        } else {
            write!(out, "  {cont_pad}")?;
        }
        self.queue_reset(out, true)?;
        queue!(
            out,
            SetForegroundColor(Self::C_HELP_DESC),
            SetAttribute(Attribute::Dim)
        )?;
        writeln!(out, "{line}")?;
        self.queue_reset(out, true)?;
        Ok(())
    }

    fn write_help_row_table_plain_line(
        &self,
        out: &mut io::Stdout,
        padded_cmd: &str,
        cont_pad: &str,
        line: &str,
        first_line: bool,
    ) -> io::Result<()> {
        if first_line {
            write!(out, "  {padded_cmd} ")?;
        } else {
            write!(out, "  {cont_pad}")?;
        }
        writeln!(out, "{line}")?;
        Ok(())
    }

    fn write_help_row_table(
        &self,
        out: &mut io::Stdout,
        padded_cmd: &str,
        cont_pad: &str,
        desc_lines: &[String],
    ) -> io::Result<()> {
        for (i, line) in desc_lines.iter().enumerate() {
            let first_line = i == 0;
            if self.use_color_stdout {
                self.write_help_row_table_colored_line(
                    out, padded_cmd, cont_pad, line, first_line,
                )?;
            } else {
                self.write_help_row_table_plain_line(out, padded_cmd, cont_pad, line, first_line)?;
            }
        }
        Ok(())
    }

    fn print_help_capture(&self) -> io::Result<()> {
        let Some(cap) = self.capture.as_ref() else {
            return Ok(());
        };
        let layout = compute_help_table_layout();
        cap.lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(render_help_body_lines(&layout));
        Ok(())
    }

    fn write_help_rows_to_stdout(
        &self,
        out: &mut io::Stdout,
        layout: &HelpTableLayout,
    ) -> io::Result<()> {
        for (cmd, desc) in REPL_HELP_ROWS {
            let desc_lines = if layout.table_ok {
                wrap_help_description(desc, layout.w_desc_table)
            } else {
                wrap_help_description(desc, layout.w_desc_stacked)
            };
            if !layout.table_ok {
                self.write_help_row_stacked(out, cmd, &desc_lines)?;
                continue;
            }
            let padded = pad_cmd_to_display_width(cmd, layout.max_cmd_w);
            self.write_help_row_table(out, &padded, &layout.cont_pad, &desc_lines)?;
        }
        Ok(())
    }

    fn write_help_footer(&self) -> io::Result<()> {
        for line in help_footer_lines() {
            self.writeln_muted_line(line.trim_start())?;
        }
        Ok(())
    }

    /// `/help`：节标题 + 命令/说明列（宽度随终端、`unicode-width` 软换行）。
    pub(crate) fn print_help(&self) -> io::Result<()> {
        if self.capture.is_some() {
            return self.print_help_capture();
        }
        let mut out = io::stdout();
        if self.use_color_stdout {
            queue!(
                out,
                SetForegroundColor(Self::C_HELP_TITLE),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(out, "内建命令")?;
        self.queue_reset(&mut out, true)?;

        let layout = compute_help_table_layout();
        self.write_help_rows_to_stdout(&mut out, &layout)?;
        writeln!(out)?;
        self.write_help_footer()
    }
}
