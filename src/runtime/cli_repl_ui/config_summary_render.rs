//! REPL `/config` 终端摘要渲染（stdout 与 TUI capture）。

use std::io::{self, Write};
use std::path::Path;

use crate::agent::per_coord::FinalPlanRequirementMode;
use crate::config::AgentConfig;

use super::CliReplStyle;
use super::config_summary;

impl CliReplStyle {
    fn print_repl_config_summary_stdout_head(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
        inner: usize,
    ) -> io::Result<()> {
        let api_base_short = config_summary::ellipsize_terminal_line(
            &cfg.llm.api_base,
            inner.saturating_sub(4).max(24),
        );
        writeln!(out)?;
        self.write_banner_subheading(out, "运行配置摘要")?;
        self.write_repl_config_summary_model_section(out, cfg, &api_base_short, no_stream)?;
        self.write_repl_config_summary_workspace_section(out, work_dir, tool_count)?;
        self.write_repl_config_summary_essentials_section(out, cfg)
    }

    fn print_repl_config_summary_stdout_tail(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
        inner: usize,
    ) -> io::Result<()> {
        self.write_repl_config_summary_planning_section(out, cfg)?;
        self.write_repl_config_summary_cursor_section(out, cfg, inner)?;
        self.write_repl_config_summary_flags_section(out, cfg)?;
        self.write_repl_config_summary_optional_services(out, cfg)?;
        self.write_banner_note_line(
            out,
            "    不含 API_KEY / web_api_bearer_token 等密钥；逐项说明见 docs/配置说明.md",
        )?;
        writeln!(out)
    }

    fn print_repl_config_summary_stdout_sections(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
        inner: usize,
    ) -> io::Result<()> {
        self.print_repl_config_summary_stdout_head(
            out, cfg, work_dir, tool_count, no_stream, inner,
        )?;
        self.print_repl_config_summary_stdout_tail(out, cfg, inner)
    }

    fn print_repl_config_summary_stdout(
        &self,
        cfg: &AgentConfig,
        work_dir: &Path,
        tool_count: usize,
        no_stream: bool,
    ) -> io::Result<()> {
        let mut out = io::stdout();
        let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
        let inner = (tw as usize).saturating_sub(4).clamp(28, 72);
        self.print_repl_config_summary_stdout_sections(
            &mut out, cfg, work_dir, tool_count, no_stream, inner,
        )?;
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
        self.print_repl_config_summary_stdout(cfg, work_dir, tool_count, no_stream)
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

    fn write_repl_config_summary_context_budget_items(
        &self,
        out: &mut io::Stdout,
        cfg: &AgentConfig,
    ) -> io::Result<()> {
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
        self.write_repl_config_summary_context_budget_items(out, cfg)?;
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
}
