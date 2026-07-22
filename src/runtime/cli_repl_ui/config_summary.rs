//! REPL `/config` 纯文本摘要行（与横幅字段对齐）。

use std::path::Path;

use crate::agent::per_coord::FinalPlanRequirementMode;
use crate::config::AgentConfig;

/// 启动横幅里 `api_base` 等过长单行：按 Unicode 标量截断并加 `…`。
pub(super) fn ellipsize_terminal_line(s: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(12);
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", s.chars().take(keep).collect::<String>())
}

fn summary_heading(lines: &mut Vec<String>, title: &str) {
    lines.push(String::new());
    lines.push(format!("  {title}"));
}

fn summary_item(lines: &mut Vec<String>, label: &str, detail: &str) {
    lines.push(format!("    · {label}  {detail}"));
}

/// REPL **`/config`**：打印关键运行配置（与启动横幅同源字段 + 若干排障项；**不**含任何密钥）。
pub(super) fn repl_config_summary_plain_lines(
    cfg: &AgentConfig,
    work_dir: &Path,
    tool_count: usize,
    no_stream: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let (tw, _) = crossterm::terminal::size().unwrap_or((72, 24));
    let inner = (tw as usize).saturating_sub(4).clamp(28, 72);
    let api_base_short =
        ellipsize_terminal_line(&cfg.llm.api_base, inner.saturating_sub(4).max(24));

    lines.push(String::new());
    summary_heading(&mut lines, "运行配置摘要");

    append_summary_model_section(&mut lines, cfg, &api_base_short, no_stream);
    append_summary_workspace_section(&mut lines, work_dir, tool_count);
    append_summary_key_limits_section(&mut lines, cfg);
    append_summary_planning_section(&mut lines, cfg);
    append_summary_integrations_section(&mut lines, cfg, inner);

    lines.push(String::new());
    lines.push(
        "    不含 API_KEY / web_api_bearer_token 等密钥；逐项说明见 docs/配置说明.md".to_string(),
    );
    lines.push(String::new());
    lines
}

fn append_summary_model_section(
    lines: &mut Vec<String>,
    cfg: &AgentConfig,
    api_base_short: &str,
    no_stream: bool,
) {
    summary_heading(lines, "模型");
    summary_item(lines, "model", cfg.llm.model.as_str());
    summary_item(lines, "api_base", api_base_short);
    summary_item(lines, "llm_http_auth", cfg.llm.llm_http_auth_mode.as_str());
    summary_item(
        lines,
        "temperature",
        &format!("{}", cfg.llm_sampling.temperature),
    );
    let seed_line = cfg
        .llm_sampling
        .llm_seed
        .map(|s| s.to_string())
        .unwrap_or_else(|| "（未设置）".to_string());
    summary_item(lines, "llm_seed", seed_line.as_str());
    let stream_line = if no_stream {
        "关闭（本进程 --no-stream）"
    } else {
        "开启（流式）"
    };
    summary_item(lines, "stream", stream_line);
}

fn append_summary_workspace_section(lines: &mut Vec<String>, work_dir: &Path, tool_count: usize) {
    summary_heading(lines, "工作区与工具");
    summary_item(lines, "工作区", &work_dir.display().to_string());
    let tools_detail = if tool_count == 0 {
        "已关闭（--no-tools）".to_string()
    } else {
        format!("{tool_count} 个可用")
    };
    summary_item(lines, "工具", tools_detail.as_str());
}

fn append_summary_key_limits_section(lines: &mut Vec<String>, cfg: &AgentConfig) {
    summary_heading(lines, "要点配置");
    summary_item(
        lines,
        "max_tokens",
        &cfg.llm_sampling.max_tokens.to_string(),
    );
    summary_item(
        lines,
        "max_message_history",
        &format!(
            "保留最近 {} 轮（user+assistant 计一轮）",
            cfg.session_ui.max_message_history
        ),
    );
    if cfg.context_pipeline.context_char_budget > 0 {
        summary_item(
            lines,
            "context_char_budget",
            &format!(
                "{}（启用按字符删旧）",
                cfg.context_pipeline.context_char_budget
            ),
        );
    }
    if cfg.llm_sampling.llm_context_tokens > 0 {
        summary_item(
            lines,
            "llm_context_tokens",
            &cfg.llm_sampling.llm_context_tokens.to_string(),
        );
        let eff = cfg.effective_context_char_budget_for_pipeline();
        if eff > 0 {
            summary_item(
                lines,
                "effective_context_char_budget",
                &format!("{}（与窗口推导取较小后的会话裁剪预算）", eff),
            );
        }
    }
    summary_item(
        lines,
        "API",
        &format!(
            "超时 {}s · 失败重试 {} 次",
            cfg.llm_http_retry.api_timeout_secs, cfg.llm_http_retry.api_max_retries
        ),
    );
    summary_item(
        lines,
        "run_command",
        &format!(
            "超时 {}s · 输出上限 {} 字",
            cfg.command_exec.command_timeout_secs, cfg.command_exec.command_max_output_len
        ),
    );
    summary_item(
        lines,
        "tool_message_max_chars",
        &cfg.tool_transcript.tool_message_max_chars.to_string(),
    );
}

fn append_summary_planning_section(lines: &mut Vec<String>, cfg: &AgentConfig) {
    let final_plan = match cfg.per_plan_policy.final_plan_requirement {
        FinalPlanRequirementMode::Never => "never",
        FinalPlanRequirementMode::WorkflowReflection => "workflow_reflection",
        FinalPlanRequirementMode::Always => "always",
    };
    summary_item(lines, "final_plan_requirement", final_plan);
    summary_item(
        lines,
        "plan_rewrite_max_attempts",
        &cfg.per_plan_policy.plan_rewrite_max_attempts.to_string(),
    );
    summary_item(
        lines,
        "planner_executor_mode",
        cfg.per_plan_policy.planner_executor_mode.as_str(),
    );
}

fn append_summary_integrations_section(lines: &mut Vec<String>, cfg: &AgentConfig, inner: usize) {
    let cursor = if cfg.cursor_rules.cursor_rules_enabled {
        let d = cfg.cursor_rules.cursor_rules_dir.trim();
        let short = if d.is_empty() {
            "（目录为空）".to_string()
        } else {
            ellipsize_terminal_line(d, inner.min(48))
        };
        format!("开启 · {}", short)
    } else {
        "关闭".to_string()
    };
    summary_item(lines, "cursor_rules", cursor.as_str());

    summary_item(
        lines,
        "materialize_deepseek_dsml_tool_calls",
        if cfg.dsml_materialize.materialize_deepseek_dsml_tool_calls {
            "开启"
        } else {
            "关闭"
        },
    );
    let explain = if cfg.tool_call_explain.tool_call_explain_enabled {
        format!(
            "开启（{}～{} 字）",
            cfg.tool_call_explain.tool_call_explain_min_chars,
            cfg.tool_call_explain.tool_call_explain_max_chars
        )
    } else {
        "关闭".to_string()
    };
    summary_item(lines, "tool_call_explain", explain.as_str());

    if cfg.session_ui.tui_load_session_on_start {
        summary_item(
            lines,
            "会话恢复",
            "启动时加载 .crabmate/tui_session.json（若存在）",
        );
    }
    if cfg.mcp_client.mcp_enabled && !cfg.mcp_client.mcp_command.trim().is_empty() {
        summary_item(lines, "MCP", "已启用（stdio）");
    }
    if cfg.long_term_memory.long_term_memory_enabled {
        summary_item(lines, "long_term_memory", "已启用");
    }
}
