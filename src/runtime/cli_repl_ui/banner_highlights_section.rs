//! REPL 启动横幅「要点配置」分节（从 `mod.rs` 拆出以降低单文件行数棘轮）。

use std::io::{self, Write};

use crossterm::QueueableCommand;

use crate::config::{AgentConfig, PlannerExecutorMode};

use super::CliReplStyle;

pub(super) fn write_banner_highlights_core_limits<W: Write + QueueableCommand>(
    style: &CliReplStyle,
    w: &mut W,
    cfg: &AgentConfig,
) -> io::Result<()> {
    style.write_banner_subheading(w, "要点配置")?;
    style.write_banner_item(w, "max_tokens", &cfg.llm_sampling.max_tokens.to_string())?;
    style.write_banner_item(
        w,
        "max_message_history",
        &format!(
            "保留最近 {} 轮（user+assistant 计一轮）",
            cfg.session_ui.max_message_history
        ),
    )?;
    style.write_banner_item(
        w,
        "API",
        &format!(
            "超时 {}s · 失败重试 {} 次",
            cfg.llm_http_retry.api_timeout_secs, cfg.llm_http_retry.api_max_retries
        ),
    )?;
    style.write_banner_item(
        w,
        "run_command",
        &format!(
            "超时 {}s · 输出上限 {} 字",
            cfg.command_exec.command_timeout_secs, cfg.command_exec.command_max_output_len
        ),
    )?;
    Ok(())
}

pub(super) fn write_banner_highlights_staged_and_planner<W: Write + QueueableCommand>(
    style: &CliReplStyle,
    w: &mut W,
    cfg: &AgentConfig,
) -> io::Result<()> {
    // L1 硬编码：staged_planning 配置字段已删除
    let bypass = "关闭（门控放行一律 PlannedStep）";
    style.write_banner_item(w, "staged_plan_intent_gate_advisory_bypass", bypass)?;
    style.write_banner_item(
        w,
        "staged_plan_feedback_mode",
        "patch_planner", // L1 硬编码：StagedPlanFeedbackMode::PatchPlanner
    )?;
    if cfg.per_plan_policy.planner_executor_mode != PlannerExecutorMode::SingleAgent {
        style.write_banner_item(
            w,
            "planner_executor_mode",
            cfg.per_plan_policy.planner_executor_mode.as_str(),
        )?;
    }
    Ok(())
}

pub(super) fn write_banner_highlights_optional_flags<W: Write + QueueableCommand>(
    style: &CliReplStyle,
    w: &mut W,
    cfg: &AgentConfig,
) -> io::Result<()> {
    if cfg.session_ui.tui_load_session_on_start {
        style.write_banner_item(
            w,
            "会话恢复",
            "启动时加载 .crabmate/tui_session.json（若存在）",
        )?;
    }
    if cfg.mcp_client.mcp_enabled && !cfg.mcp_client.mcp_command.trim().is_empty() {
        style.write_banner_item(w, "MCP", "已启用（stdio）")?;
    }
    if cfg.long_term_memory.long_term_memory_enabled {
        style.write_banner_item(w, "long_term_memory", "已启用")?;
    }
    Ok(())
}
