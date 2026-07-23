//! REPL 终端样式：**集中在 [`CliReplStyle`]**（配色、是否启用 ANSI）；尊重 **`NO_COLOR`**，非 TTY 时不写入转义序列。
//!
//! **捕获模式**：[`CliReplStyle::new_tui_capture`] 将本应写入终端的行追加到缓冲区（纯文本、无 ANSI），供全屏 TUI 写入 transcript。

mod banner_highlights_section;
mod config_summary;
mod tables;

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tables::{
    HELP_DESC_MIN, HELP_GAP, HELP_LEFT, REPL_HELP_ROWS, pad_cmd_to_display_width,
    spaces_to_display_width, wrap_help_description,
};
use unicode_width::UnicodeWidthStr;

use crate::agent::per_coord::FinalPlanRequirementMode;
use crate::config::{AgentConfig, LlmHttpAuthMode};

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

    fn write_help_row_table(
        &self,
        out: &mut io::Stdout,
        padded_cmd: &str,
        cont_pad: &str,
        desc_lines: &[String],
    ) -> io::Result<()> {
        for (i, line) in desc_lines.iter().enumerate() {
            if self.use_color_stdout {
                queue!(
                    out,
                    SetForegroundColor(Self::C_HELP_CMD),
                    SetAttribute(Attribute::Bold)
                )?;
            }
            if i == 0 {
                write!(out, "  {padded_cmd} ")?;
            } else {
                write!(out, "  {cont_pad}")?;
            }
            self.queue_reset(out, true)?;
            if self.use_color_stdout {
                queue!(
                    out,
                    SetForegroundColor(Self::C_HELP_DESC),
                    SetAttribute(Attribute::Dim)
                )?;
            }
            writeln!(out, "{line}")?;
            self.queue_reset(out, true)?;
        }
        Ok(())
    }

    fn write_banner_subheading<W: Write + QueueableCommand>(
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

    fn write_banner_item<W: Write + QueueableCommand>(
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

    fn write_banner_note_line<W: Write + QueueableCommand>(
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

    /// 顶栏：**FIGlet 风格 CrabMate**（6 行 ASCII；行内已含缩进，**`NO_COLOR`** 不乱码）。
    fn write_banner_art_header<W: Write + QueueableCommand>(&self, w: &mut W) -> io::Result<()> {
        for line in BANNER_CRABMATE_ART {
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
            "    /clear  /model（·set） /api-base（·set） /models（list·choose） /api-key  /agent（list·set） /config  /doctor  /probe  /mcp  /version  /workspace（/cd） /skills（list） /tools  /export  /save-session  /help  /?  · Tab 补全",
        )?;
        self.write_banner_note_line(
            w,
            "    行首 $ → 本地 shell（bash#:）；quit / exit / Ctrl+D 退出",
        )?;
        self.write_banner_note_line(w, "    非白名单 run_command：y 一次 / a 本会话允许该命令名")?;
        if cfg.llm.llm_http_auth_mode == LlmHttpAuthMode::Bearer && !repl_llm_bearer_key_ready {
            self.write_banner_note_line(
                w,
                "    提示：未检测到环境变量 API_KEY；对话前请执行 /api-key set <密钥>（仅本进程）或 export API_KEY 后重启。",
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
    /// `repl_llm_bearer_key_ready`：为 true 时不在横幅打印「未设 API_KEY」提示（启动时环境变量非空即可为 true；REPL 内设置密钥后横幅不会自动刷新）。
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

    pub(crate) fn print_repl_config_summary(
        &self,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
    ) -> io::Result<()> {
        if let Some(cap) = &self.capture {
            let plain = config_summary::repl_config_summary_plain_lines(
                cfg, work_dir, tool_count, no_stream,
            );
            cap.lock().unwrap_or_else(|e| e.into_inner()).extend(plain);
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
        self.write_banner_subheading(&mut out, "运行配置摘要")?;

        self.write_repl_config_summary_model_section(&mut out, cfg, &api_base_short, no_stream)?;
        self.write_repl_config_summary_workspace_section(&mut out, work_dir, tool_count)?;
        self.write_repl_config_summary_essentials_section(&mut out, cfg)?;
        self.write_repl_config_summary_planning_section(&mut out, cfg)?;
        self.write_repl_config_summary_cursor_section(&mut out, cfg, inner)?;
        self.write_repl_config_summary_flags_section(&mut out, cfg)?;
        self.write_repl_config_summary_optional_services(&mut out, cfg)?;

        self.write_banner_note_line(
            &mut out,
            "    不含 API_KEY / web_api_bearer_token 等密钥；逐项说明见 docs/配置说明.md",
        )?;
        writeln!(out)?;
        out.flush()
    }

    fn write_repl_config_summary_model_section(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
        api_base_short: &str,
        no_stream: bool,
    ) -> io::Result<()> {
        self.write_banner_subheading(out, "模型")?;
        self.write_banner_item(out, "model", &cfg.llm.model)?;
        self.write_banner_item(out, "api_base", api_base_short)?;
        self.write_banner_item(out, "llm_http_auth", cfg.llm.llm_http_auth_mode.as_str())?;
        self.write_banner_item(
            out,
            "temperature",
            &format!("{}", cfg.llm_sampling.temperature),
        )?;
        let seed_line = cfg
            .llm_sampling
            .llm_seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "（未设置）".to_string());
        self.write_banner_item(out, "llm_seed", &seed_line)?;
        let stream_line = if no_stream {
            "关闭（本进程 --no-stream）"
        } else {
            "开启（流式）"
        };
        self.write_banner_item(out, "stream", stream_line)?;
        Ok(())
    }

    fn write_repl_config_summary_workspace_section(
        &self,
        out: &mut io::Stdout,
        work_dir: &Path,
        tool_count: usize,
    ) -> io::Result<()> {
        self.write_banner_subheading(out, "工作区与工具")?;
        self.write_banner_item(out, "工作区", &work_dir.display().to_string())?;
        let tools_detail = if tool_count == 0 {
            "已关闭（--no-tools）".to_string()
        } else {
            format!("{tool_count} 个可用")
        };
        self.write_banner_item(out, "工具", &tools_detail)?;
        Ok(())
    }

    fn write_repl_config_summary_essentials_section(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
        self.write_banner_subheading(out, "要点配置")?;
        self.write_banner_item(out, "max_tokens", &cfg.llm_sampling.max_tokens.to_string())?;
        self.write_banner_item(
            out,
            "max_message_history",
            &format!(
                "保留最近 {} 轮（user+assistant 计一轮）",
                cfg.session_ui.max_message_history
            ),
        )?;
        if cfg.context_pipeline.context_char_budget > 0 {
            self.write_banner_item(
                out,
                "context_char_budget",
                &format!(
                    "{}（启用按字符删旧）",
                    cfg.context_pipeline.context_char_budget
                ),
            )?;
        }
        if cfg.llm_sampling.llm_context_tokens > 0 {
            self.write_banner_item(
                out,
                "llm_context_tokens",
                &cfg.llm_sampling.llm_context_tokens.to_string(),
            )?;
            let eff = cfg.effective_context_char_budget_for_pipeline();
            if eff > 0 {
                self.write_banner_item(
                    out,
                    "effective_context_char_budget",
                    &format!("{}（与窗口推导取较小后的会话裁剪预算）", eff),
                )?;
            }
        }
        self.write_banner_item(
            out,
            "API",
            &format!(
                "超时 {}s · 失败重试 {} 次",
                cfg.llm_http_retry.api_timeout_secs, cfg.llm_http_retry.api_max_retries
            ),
        )?;
        self.write_banner_item(
            out,
            "run_command",
            &format!(
                "超时 {}s · 输出上限 {} 字",
                cfg.command_exec.command_timeout_secs, cfg.command_exec.command_max_output_len
            ),
        )?;
        self.write_banner_item(
            out,
            "tool_message_max_chars",
            &cfg.tool_transcript.tool_message_max_chars.to_string(),
        )?;
        Ok(())
    }

    fn write_repl_config_summary_planning_section(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
        let final_plan = match cfg.per_plan_policy.final_plan_requirement {
            FinalPlanRequirementMode::Never => "never",
            FinalPlanRequirementMode::WorkflowReflection => "workflow_reflection",
            FinalPlanRequirementMode::Always => "always",
        };
        self.write_banner_item(out, "final_plan_requirement", final_plan)?;
        self.write_banner_item(
            out,
            "plan_rewrite_max_attempts",
            &cfg.per_plan_policy.plan_rewrite_max_attempts.to_string(),
        )?;
        self.write_banner_item(
            out,
            "planner_executor_mode",
            cfg.per_plan_policy.planner_executor_mode.as_str(),
        )?;
        Ok(())
    }

    fn write_repl_config_summary_cursor_section(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
        inner: usize,
    ) -> io::Result<()> {
        let cursor = if cfg.cursor_rules.cursor_rules_enabled {
            let d = cfg.cursor_rules.cursor_rules_dir.trim();
            let short = if d.is_empty() {
                "（目录为空）".to_string()
            } else {
                config_summary::ellipsize_terminal_line(d, inner.min(48))
            };
            format!("开启 · {}", short)
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(out, "cursor_rules", &cursor)?;
        Ok(())
    }

    fn write_repl_config_summary_flags_section(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
        self.write_banner_item(
            out,
            "materialize_deepseek_dsml_tool_calls",
            if cfg.dsml_materialize.materialize_deepseek_dsml_tool_calls {
                "开启"
            } else {
                "关闭"
            },
        )?;
        let explain = if cfg.tool_call_explain.tool_call_explain_enabled {
            format!(
                "开启（{}～{} 字）",
                cfg.tool_call_explain.tool_call_explain_min_chars,
                cfg.tool_call_explain.tool_call_explain_max_chars
            )
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(out, "tool_call_explain", &explain)?;
        Ok(())
    }

    fn write_repl_config_summary_optional_services(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
        if cfg.session_ui.tui_load_session_on_start {
            self.write_banner_item(
                out,
                "会话恢复",
                "启动时加载 .crabmate/tui_session.json（若存在）",
            )?;
        }
        if cfg.mcp_client.mcp_enabled && !cfg.mcp_client.mcp_command.trim().is_empty() {
            self.write_banner_item(out, "MCP", "已启用（stdio）")?;
        }
        if cfg.long_term_memory.long_term_memory_enabled {
            self.write_banner_item(out, "long_term_memory", "已启用")?;
        }
        Ok(())
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

    /// `/help`：节标题 + 命令/说明列（宽度随终端、`unicode-width` 软换行）。
    fn print_help_capture(&self) -> io::Result<()> {
        let Some(cap) = self.capture.as_ref() else {
            return Ok(());
        };
        let mut lines: Vec<String> = Vec::new();
        lines.push(String::new());
        lines.push("  内建命令".to_string());
        let rows = REPL_HELP_ROWS;
        let (tw, _) = crossterm::terminal::size().unwrap_or((80, 24));
        let inner = tw as usize;
        let max_cmd_w = rows.iter().map(|(c, _)| c.width()).max().unwrap_or(0);
        let table_ok = inner >= HELP_LEFT + max_cmd_w + HELP_GAP + HELP_DESC_MIN;
        let w_desc_table = inner
            .saturating_sub(HELP_LEFT + max_cmd_w + HELP_GAP)
            .max(1);
        let w_desc_stacked = inner.saturating_sub(HELP_LEFT).max(1);
        for (cmd, desc) in rows {
            let desc_lines = if table_ok {
                wrap_help_description(desc, w_desc_table)
            } else {
                wrap_help_description(desc, w_desc_stacked)
            };
            if !table_ok {
                lines.push(format!("  {cmd}"));
                for d in &desc_lines {
                    lines.push(format!("  {d}"));
                }
                continue;
            }
            let padded = pad_cmd_to_display_width(cmd, max_cmd_w);
            let cont_pad = spaces_to_display_width(max_cmd_w + HELP_GAP);
            for (i, line) in desc_lines.iter().enumerate() {
                if i == 0 {
                    lines.push(format!("  {padded} {line}"));
                } else {
                    lines.push(format!("  {cont_pad}{line}"));
                }
            }
        }
        lines.push(String::new());
        lines.push(
            "  「我:」下光标前为 /… 时按 Tab 可补全内建命令与 /export、/save-session、/mcp 子命令；bash#: 下不补全"
                .to_string(),
        );
        lines.push("  退出：quit · exit · Ctrl+D".to_string());
        cap.lock().unwrap_or_else(|e| e.into_inner()).extend(lines);
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

        let rows = REPL_HELP_ROWS;

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
                self.write_help_row_stacked(&mut out, cmd, &desc_lines)?;
                continue;
            }

            let padded = pad_cmd_to_display_width(cmd, max_cmd_w);
            self.write_help_row_table(&mut out, &padded, &cont_pad, &desc_lines)?;
        }
        writeln!(out)?;
        self.writeln_muted_line(
            "「我:」下光标前为 /… 时按 Tab 可补全内建命令与 /export、/save-session、/mcp 子命令；bash#: 下不补全",
        )?;
        self.writeln_muted_line("退出：quit · exit · Ctrl+D")?;
        Ok(())
    }
}
