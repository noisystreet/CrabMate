//! REPL 终端样式：**集中在 [`CliReplStyle`]**（配色、是否启用 ANSI）；尊重 **`NO_COLOR`**，非 TTY 时不写入转义序列。

use std::io::{self, IsTerminal, Write};
use std::path::Path;

use crate::agent::per_coord::FinalPlanRequirementMode;
use crate::config::{AgentConfig, PlannerExecutorMode};

use crossterm::{
    QueueableCommand, queue,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

/// 启动横幅里 `api_base` 等过长单行：按 Unicode 标量截断并加 `…`。
fn ellipsize_terminal_line(s: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(12);
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", s.chars().take(keep).collect::<String>())
}

/// REPL 顶栏 FIGlet 风格 **CrabMate**（固定 6 行 ASCII；`r"..."` 保留 `\`）。
const BANNER_CRABMATE_ART: &[&str] = &[
    r"  ______ .______          ___      .______   .___  ___.      ___   .___________. _______ ",
    r" /      ||   _  \        /   \     |   _  \  |   \/   |     /   \  |           ||   ____|",
    r"|  ,----'|  |_)  |      /  ^  \    |  |_)  | |  \  /  |    /  ^  \ `---|  |----`|  |__   ",
    r"|  |     |      /      /  /_\  \   |   _  <  |  |\/|  |   /  /_\  \    |  |     |   __|  ",
    r"|  `----.|  |\  \----./  _____  \  |  |_)  | |  |  |  |  /  _____  \   |  |     |  |____ ",
    r" \______|| _| `._____/__/     \__\ |______/  |__|  |__| /__/     \__\  |__|     |_______|",
];

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

    /// 启动横幅：**FIGlet CrabMate** 顶栏 + **模型状态**、**内建命令**、**要点配置**分节（与 `/help` 同色阶；**`NO_COLOR`** 下纯文本）。
    pub(crate) fn print_banner(
        &self,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
    ) -> io::Result<()> {
        let mut out = io::stdout();
        let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
        let inner = (tw as usize).saturating_sub(4).clamp(28, 72);
        let api_base_short =
            ellipsize_terminal_line(&cfg.api_base, inner.saturating_sub(4).max(24));

        writeln!(out)?;
        self.write_banner_art_header(&mut out)?;

        self.write_banner_subheading(&mut out, "模型")?;
        self.write_banner_item(&mut out, "model", &cfg.model)?;
        self.write_banner_item(&mut out, "api_base", &api_base_short)?;
        self.write_banner_item(&mut out, "llm_http_auth", cfg.llm_http_auth_mode.as_str())?;
        self.write_banner_item(&mut out, "temperature", &format!("{}", cfg.temperature))?;
        let seed_line = cfg
            .llm_seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "（未设置，请求不带 seed）".to_string());
        self.write_banner_item(&mut out, "llm_seed", &seed_line)?;
        let stream_line = if no_stream {
            "关闭（本进程 --no-stream）"
        } else {
            "开启（流式）"
        };
        self.write_banner_item(&mut out, "stream", stream_line)?;

        self.write_banner_subheading(&mut out, "工作区与工具")?;
        self.write_banner_item(&mut out, "工作区", &work_dir.display().to_string())?;
        let tools_detail = if tool_count == 0 {
            "已关闭（--no-tools）".to_string()
        } else {
            format!("{tool_count} 个可用")
        };
        self.write_banner_item(&mut out, "工具", &tools_detail)?;

        self.write_banner_subheading(&mut out, "内建命令")?;
        self.write_banner_note_line(
            &mut out,
            "    /clear  /model  /models  /config  /doctor  /probe  /workspace（/cd） /tools  /export  /save-session  /help  /?  · Tab 补全",
        )?;
        self.write_banner_note_line(
            &mut out,
            "    行首 $ → 本地 shell（bash#:）；quit / exit / Ctrl+D 退出",
        )?;
        self.write_banner_note_line(
            &mut out,
            "    非白名单 run_command：y 一次 / a 本会话允许该命令名",
        )?;

        self.write_banner_subheading(&mut out, "要点配置")?;
        self.write_banner_item(&mut out, "max_tokens", &cfg.max_tokens.to_string())?;
        self.write_banner_item(
            &mut out,
            "max_message_history",
            &format!(
                "保留最近 {} 轮（user+assistant 计一轮）",
                cfg.max_message_history
            ),
        )?;

        self.write_banner_item(
            &mut out,
            "API",
            &format!(
                "超时 {}s · 失败重试 {} 次",
                cfg.api_timeout_secs, cfg.api_max_retries
            ),
        )?;
        self.write_banner_item(
            &mut out,
            "run_command",
            &format!(
                "超时 {}s · 输出上限 {} 字",
                cfg.command_timeout_secs, cfg.command_max_output_len
            ),
        )?;

        let staged = if cfg.staged_plan_execution {
            format!("开启（{}）", cfg.staged_plan_feedback_mode.as_str())
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(&mut out, "staged_plan_execution", &staged)?;

        if cfg.planner_executor_mode != PlannerExecutorMode::SingleAgent {
            self.write_banner_item(
                &mut out,
                "planner_executor_mode",
                cfg.planner_executor_mode.as_str(),
            )?;
        }

        if cfg.tui_load_session_on_start {
            self.write_banner_item(
                &mut out,
                "会话恢复",
                "启动时加载 .crabmate/tui_session.json（若存在）",
            )?;
        }

        if cfg.mcp_enabled && !cfg.mcp_command.trim().is_empty() {
            self.write_banner_item(&mut out, "MCP", "已启用（stdio）")?;
        }

        if cfg.long_term_memory_enabled {
            self.write_banner_item(&mut out, "long_term_memory", "已启用")?;
        }

        writeln!(out)?;
        out.flush()
    }

    /// REPL **`/config`**：打印关键运行配置（与启动横幅同源字段 + 若干排障项；**不**含任何密钥）。
    pub(crate) fn print_repl_config_summary(
        &self,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
    ) -> io::Result<()> {
        let mut out = io::stdout();
        let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
        let inner = (tw as usize).saturating_sub(4).clamp(28, 72);
        let api_base_short =
            ellipsize_terminal_line(&cfg.api_base, inner.saturating_sub(4).max(24));

        writeln!(out)?;
        self.write_banner_subheading(&mut out, "运行配置摘要")?;

        self.write_banner_subheading(&mut out, "模型")?;
        self.write_banner_item(&mut out, "model", &cfg.model)?;
        self.write_banner_item(&mut out, "api_base", &api_base_short)?;
        self.write_banner_item(&mut out, "llm_http_auth", cfg.llm_http_auth_mode.as_str())?;
        self.write_banner_item(&mut out, "temperature", &format!("{}", cfg.temperature))?;
        let seed_line = cfg
            .llm_seed
            .map(|s| s.to_string())
            .unwrap_or_else(|| "（未设置）".to_string());
        self.write_banner_item(&mut out, "llm_seed", &seed_line)?;
        let stream_line = if no_stream {
            "关闭（本进程 --no-stream）"
        } else {
            "开启（流式）"
        };
        self.write_banner_item(&mut out, "stream", stream_line)?;

        self.write_banner_subheading(&mut out, "工作区与工具")?;
        self.write_banner_item(&mut out, "工作区", &work_dir.display().to_string())?;
        let tools_detail = if tool_count == 0 {
            "已关闭（--no-tools）".to_string()
        } else {
            format!("{tool_count} 个可用")
        };
        self.write_banner_item(&mut out, "工具", &tools_detail)?;

        self.write_banner_subheading(&mut out, "要点配置")?;
        self.write_banner_item(&mut out, "max_tokens", &cfg.max_tokens.to_string())?;
        self.write_banner_item(
            &mut out,
            "max_message_history",
            &format!(
                "保留最近 {} 轮（user+assistant 计一轮）",
                cfg.max_message_history
            ),
        )?;
        if cfg.context_char_budget > 0 {
            self.write_banner_item(
                &mut out,
                "context_char_budget",
                &format!("{}（启用按字符删旧）", cfg.context_char_budget),
            )?;
        }
        self.write_banner_item(
            &mut out,
            "API",
            &format!(
                "超时 {}s · 失败重试 {} 次",
                cfg.api_timeout_secs, cfg.api_max_retries
            ),
        )?;
        self.write_banner_item(
            &mut out,
            "run_command",
            &format!(
                "超时 {}s · 输出上限 {} 字",
                cfg.command_timeout_secs, cfg.command_max_output_len
            ),
        )?;
        self.write_banner_item(
            &mut out,
            "tool_message_max_chars",
            &cfg.tool_message_max_chars.to_string(),
        )?;

        let final_plan = match cfg.final_plan_requirement {
            FinalPlanRequirementMode::Never => "never",
            FinalPlanRequirementMode::WorkflowReflection => "workflow_reflection",
            FinalPlanRequirementMode::Always => "always",
        };
        self.write_banner_item(&mut out, "final_plan_requirement", final_plan)?;
        self.write_banner_item(
            &mut out,
            "plan_rewrite_max_attempts",
            &cfg.plan_rewrite_max_attempts.to_string(),
        )?;
        self.write_banner_item(
            &mut out,
            "planner_executor_mode",
            cfg.planner_executor_mode.as_str(),
        )?;

        let staged = if cfg.staged_plan_execution {
            format!("开启（{}）", cfg.staged_plan_feedback_mode.as_str())
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(&mut out, "staged_plan_execution", &staged)?;
        let staged_cli = if cfg.staged_plan_cli_show_planner_stream {
            "开启（CLI 规划轮打印模型 stdout）"
        } else {
            "关闭（CLI 规划轮不打印模型 stdout）"
        };
        self.write_banner_item(&mut out, "staged_plan_cli_show_planner_stream", staged_cli)?;

        let cursor = if cfg.cursor_rules_enabled {
            let d = cfg.cursor_rules_dir.trim();
            let short = if d.is_empty() {
                "（目录为空）".to_string()
            } else {
                ellipsize_terminal_line(d, inner.min(48))
            };
            format!("开启 · {}", short)
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(&mut out, "cursor_rules", &cursor)?;

        self.write_banner_item(
            &mut out,
            "materialize_deepseek_dsml_tool_calls",
            if cfg.materialize_deepseek_dsml_tool_calls {
                "开启"
            } else {
                "关闭"
            },
        )?;

        let explain = if cfg.tool_call_explain_enabled {
            format!(
                "开启（{}～{} 字）",
                cfg.tool_call_explain_min_chars, cfg.tool_call_explain_max_chars
            )
        } else {
            "关闭".to_string()
        };
        self.write_banner_item(&mut out, "tool_call_explain", &explain)?;

        if cfg.tui_load_session_on_start {
            self.write_banner_item(
                &mut out,
                "会话恢复",
                "启动时加载 .crabmate/tui_session.json（若存在）",
            )?;
        }
        if cfg.mcp_enabled && !cfg.mcp_command.trim().is_empty() {
            self.write_banner_item(&mut out, "MCP", "已启用（stdio）")?;
        }
        if cfg.long_term_memory_enabled {
            self.write_banner_item(&mut out, "long_term_memory", "已启用")?;
        }

        self.write_banner_note_line(
            &mut out,
            "    不含 API_KEY / web_api_bearer_token 等密钥；逐项说明见 docs/CONFIGURATION.md",
        )?;
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

    /// 成功反馈行：着色 TTY 下前缀 **`✓`**；**`NO_COLOR`** 或非 TTY 下为 **`[ok]`**，避免缺字字体显示为乱码。
    pub(crate) fn print_success(&self, msg: &str) -> io::Result<()> {
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
    pub(crate) fn print_help(&self) -> io::Result<()> {
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

        let rows: &[(&str, &str)] = &[
            ("/clear", "清空对话，仅保留当前 system 提示词"),
            ("/model", "显示 model、api_base、temperature、llm_seed"),
            (
                "/config",
                "打印关键运行配置摘要（与启动横幅同源字段；不含密钥）",
            ),
            (
                "/doctor",
                "一页环境诊断（同 crabmate doctor；不要求 API_KEY）",
            ),
            (
                "/probe",
                "探测 api_base 的 GET …/models 连通性（同 crabmate probe；需 bearer 时依赖 API_KEY）",
            ),
            (
                "/models",
                "列出 GET …/models 返回的模型 id（同 crabmate models；需 bearer 时依赖 API_KEY）",
            ),
            ("/workspace", "显示当前工作区"),
            (
                "/workspace <路径>",
                "切换工作区（须为已存在目录，别名 /cd）",
            ),
            ("/tools", "列出当前加载的工具名"),
            (
                "/export [json|markdown|both]",
                "导出当前内存对话到 .crabmate/exports/（与 Web 同形 JSON/Markdown）",
            ),
            (
                "/save-session [json|markdown|both]",
                "从磁盘会话文件导出到 .crabmate/exports/（同 crabmate save-session；默认 tui_session.json）",
            ),
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
        self.writeln_muted_line(
            "「我:」下光标前为 /… 时按 Tab 可补全内建命令与 /export、/save-session 格式；bash#: 下不补全",
        )?;
        self.writeln_muted_line("退出：quit · exit · Ctrl+D")?;
        Ok(())
    }
}
