//! REPL 启动横幅与共用 write_banner_* 行渲染。

use std::io::{self, Write};
use std::path::Path;

use crossterm::QueueableCommand;
use crossterm::queue;
use crossterm::style::{Attribute, SetAttribute, SetForegroundColor};

use crate::config::{AgentConfig, LlmHttpAuthMode};

use super::CliReplStyle;
use super::banner_highlights_section;
use super::config_summary;

impl CliReplStyle {
    pub(super) fn write_banner_subheading<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        title: &str,
    ) -> io::Result<()> {
        writeln!(w)?;
        if self.use_color_stdout {
            queue!(
                w,
                SetForegroundColor(Self::C_HELP_TITLE),
                SetAttribute(Attribute::Bold)
            )?;
        }
        writeln!(w, "  {title}")?;
        self.queue_reset(w, true)?;
        Ok(())
    }

    pub(super) fn write_banner_item<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        label: &str,
        detail: &str,
    ) -> io::Result<()> {
        if !self.use_color_stdout {
            writeln!(w, "    · {label}  {detail}")?;
            return Ok(());
        }
        write!(w, "    · ")?;
        queue!(w, SetForegroundColor(Self::C_HELP_CMD))?;
        write!(w, "{label}")?;
        self.queue_reset(w, true)?;
        queue!(
            w,
            SetForegroundColor(Self::C_MUTED),
            SetAttribute(Attribute::Dim)
        )?;
        writeln!(w, "  {detail}")?;
        self.queue_reset(w, true)?;
        Ok(())
    }

    pub(super) fn write_banner_note_line<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        line: &str,
    ) -> io::Result<()> {
        if self.use_color_stdout {
            queue!(
                w,
                SetForegroundColor(Self::C_MUTED),
                SetAttribute(Attribute::Dim)
            )?;
        }
        writeln!(w, "{line}")?;
        self.queue_reset(w, true)?;
        Ok(())
    }

    fn write_banner_art_header<W: Write + QueueableCommand>(&self, w: &mut W) -> io::Result<()> {
        for line in super::BANNER_CRABMATE_ART {
            if self.use_color_stdout {
                queue!(
                    w,
                    SetForegroundColor(Self::C_BANNER_TITLE),
                    SetAttribute(Attribute::Bold)
                )?;
            }
            writeln!(w, "{line}")?;
            self.queue_reset(w, true)?;
        }
        Ok(())
    }

    fn print_banner_model_section<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        cfg: &AgentConfig,
        api_base_short: &str,
        no_stream: bool,
    ) -> io::Result<()> {
        self.write_banner_subheading(w, "模型")?;
        self.write_banner_item(w, "model", &cfg.llm.model)?;
        self.write_banner_item(w, "api_base", api_base_short)?;
        self.write_banner_item(w, "llm_http_auth", cfg.llm.llm_http_auth_mode.as_str())?;
        self.write_banner_item(
            w,
            "temperature",
            &format!("{}", cfg.llm_sampling.temperature),
        )?;
        let seed_line = cfg
            .llm_sampling
            .llm_seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "（未设置，请求不带 seed）".to_string());
        self.write_banner_item(w, "llm_seed", &seed_line)?;
        let stream_line = if no_stream {
            "关闭（本进程 --no-stream）"
        } else {
            "开启（流式）"
        };
        self.write_banner_item(w, "stream", stream_line)?;
        Ok(())
    }

    fn print_banner_workspace_section<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        work_dir: &Path,
        tool_count: usize,
    ) -> io::Result<()> {
        self.write_banner_subheading(w, "工作区与工具")?;
        self.write_banner_item(w, "工作区", &work_dir.display().to_string())?;
        let tools_detail = if tool_count == 0 {
            "已关闭（--no-tools）".to_string()
        } else {
            format!("{tool_count} 个可用")
        };
        self.write_banner_item(w, "工具", &tools_detail)?;
        Ok(())
    }

    fn print_banner_builtin_section<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        cfg: &AgentConfig,
        repl_llm_bearer_key_ready: bool,
    ) -> io::Result<()> {
        self.write_banner_subheading(w, "内建命令")?;
        self.write_banner_note_line(
            w,
            "    /clear  /model（·set） /api-base（·set） /models（list·choose） /api-key  /agent（list·set） /mode（ask·plan·act） /config  /doctor  /probe  /mcp  /version  /workspace（/cd） /skills（list） /<skill-id>（强制技能） /tools  /export  /save-session  /help  /?  · Tab 补全",
        )?;
        self.write_banner_note_line(
            w,
            "    行首 $ → 本地 shell（bash#:）；quit / exit / Ctrl+D 退出",
        )?;
        self.write_banner_note_line(w, "    非白名单 run_command：y 一次 / a 本会话允许该命令名")?;
        if cfg.llm.llm_http_auth_mode == LlmHttpAuthMode::Bearer && !repl_llm_bearer_key_ready {
            self.write_banner_note_line(
                w,
                "    提示：未检测到 API_KEY 或系统钥匙串密钥；对话前请 /api-key set <密钥>（仅本进程）、export API_KEY，或在 Web 侧栏保存密钥。",
            )?;
        }
        Ok(())
    }

    fn print_banner_highlights_section<W: Write + QueueableCommand>(
        &self,
        w: &mut W,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
        banner_highlights_section::write_banner_highlights_core_limits(self, w, cfg)?;
        banner_highlights_section::write_banner_highlights_optional_flags(self, w, cfg)?;
        Ok(())
    }

    /// 启动横幅：**FIGlet CrabMate** 顶栏 + **模型状态**、**内建命令**、**要点配置**分节（与 `/help` 同色阶；**`NO_COLOR`** 下纯文本）。
    pub(crate) fn print_banner(
        &self,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
        repl_llm_bearer_key_ready: bool,
    ) -> io::Result<()> {
        if self.capture.is_some() {
            return Ok(());
        }
        let mut out = io::stdout();
        let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
        let inner = (tw as usize).saturating_sub(4).clamp(28, 72);
        let api_base_short = config_summary::ellipsize_terminal_line(
            &cfg.llm.api_base,
            inner.saturating_sub(4).max(24),
        );

        writeln!(out)?;
        self.write_banner_art_header(&mut out)?;

        self.print_banner_model_section(&mut out, cfg, &api_base_short, no_stream)?;
        self.print_banner_workspace_section(&mut out, work_dir, tool_count)?;
        self.print_banner_builtin_section(&mut out, cfg, repl_llm_bearer_key_ready)?;
        self.print_banner_highlights_section(&mut out, cfg)?;

        writeln!(out)?;
        out.flush()
    }
}
