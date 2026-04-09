//! 进程内全局工具调用轻量统计：每次工具完结记录 `ok` / `error_code`（与 `crabmate_tool` 语义一致），
//! 在新会话首条 `system` 末尾可选附加短 Markdown 提示（不落盘、不按会话分桶）。

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

use crate::config::AgentConfig;
use crate::tool_result::{NormalizedToolEnvelope, ToolEnvelopeContext, parse_legacy_output};

struct ToolStatEvent {
    tool: String,
    ok: bool,
    error_code: Option<String>,
}

static TOOL_STAT_EVENTS: LazyLock<Mutex<VecDeque<ToolStatEvent>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// 与 SSE / `role: tool` 写入共用解析路径，在工具结果已定稿时调用。
pub(crate) fn record_tool_outcome(
    cfg: &AgentConfig,
    tool_name: &str,
    result_raw: &str,
    tool_summary: Option<String>,
    envelope_ctx: Option<&ToolEnvelopeContext<'_>>,
) {
    if !cfg.agent_tool_stats_enabled {
        return;
    }
    let parsed = parse_legacy_output(tool_name, result_raw);
    let summary = tool_summary.unwrap_or_else(|| format!("tool: {tool_name}"));
    let norm = NormalizedToolEnvelope::from_tool_run(
        tool_name,
        summary,
        &parsed,
        result_raw,
        envelope_ctx,
    );
    let cap = cfg.agent_tool_stats_window_events.max(1);
    let mut q = TOOL_STAT_EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    q.push_back(ToolStatEvent {
        tool: norm.name.clone(),
        ok: norm.ok,
        error_code: norm.error_code.clone(),
    });
    while q.len() > cap {
        q.pop_front();
    }
}

/// 在已解析好的首条 `system` 正文后附加统计提示（未启用或无内容则返回 `base` 克隆）。
pub fn augment_system_prompt(base: &str, cfg: &AgentConfig) -> String {
    let mut out = base.to_string();
    if cfg.thinking_avoid_echo_system_prompt {
        let app = cfg.thinking_avoid_echo_appendix.trim();
        if !app.is_empty() {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            out.push_str(app);
        }
    }
    let Some(app) = hints_markdown(cfg) else {
        return out;
    };
    if app.is_empty() {
        return out;
    }
    format!("{out}\n\n{app}")
}

fn hints_markdown(cfg: &AgentConfig) -> Option<String> {
    if !cfg.agent_tool_stats_enabled {
        return None;
    }
    let q = TOOL_STAT_EVENTS.lock().ok()?;
    let min_s = cfg.agent_tool_stats_min_samples.max(1);
    let ratio_threshold = cfg
        .agent_tool_stats_warn_below_success_ratio
        .clamp(0.0, 1.0);

    let mut per_tool: HashMap<String, (usize, usize, HashMap<String, usize>)> = HashMap::new();
    for ev in q.iter() {
        let e = per_tool
            .entry(ev.tool.clone())
            .or_insert((0, 0, HashMap::new()));
        e.0 += 1;
        if ev.ok {
            e.1 += 1;
        } else if let Some(ref code) = ev.error_code {
            *e.2.entry(code.clone()).or_insert(0) += 1;
        } else {
            *e.2.entry("(无 error_code)".to_string()).or_insert(0) += 1;
        }
    }

    let mut keys: Vec<String> = per_tool.keys().cloned().collect();
    keys.sort();

    let mut lines: Vec<String> = Vec::new();
    for tool in keys {
        let (total, ok_c, ref err_map) = per_tool[&tool];
        if total < min_s {
            continue;
        }
        let fail_c = total.saturating_sub(ok_c);
        let success_r = ok_c as f64 / total as f64;
        if fail_c == 0 && success_r + f64::EPSILON >= ratio_threshold {
            continue;
        }
        let mut line = format!("- `{tool}`：窗口内 {total} 次，成功 {ok_c} / 失败 {fail_c}");
        if fail_c > 0 && !err_map.is_empty() {
            let mut pairs: Vec<(&String, &usize)> = err_map.iter().collect();
            pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            let top: Vec<String> = pairs
                .into_iter()
                .take(3)
                .map(|(c, n)| format!("`{c}`×{n}"))
                .collect();
            line.push_str(&format!("；常见错误：{}", top.join("、")));
        }
        lines.push(line);
    }

    if lines.is_empty() {
        return Some(String::new());
    }

    let header = "## 近期工具调用提示（进程内全局统计，仅供参考）";
    let body = lines.join("\n");
    let mut out = format!("{header}\n\n{body}");
    let max_c = cfg.agent_tool_stats_max_chars.max(64);
    let len = out.chars().count();
    if len > max_c {
        let take = max_c.saturating_sub(12);
        out = format!("{}…（已截断）", out.chars().take(take).collect::<String>());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> AgentConfig {
        let mut c = crate::load_config(None).expect("load default config");
        c.agent_tool_stats_enabled = true;
        c.agent_tool_stats_window_events = 100;
        c.agent_tool_stats_min_samples = 3;
        c.agent_tool_stats_max_chars = 4000;
        c.agent_tool_stats_warn_below_success_ratio = 0.9;
        c
    }

    fn reset_ring() {
        let mut q = TOOL_STAT_EVENTS.lock().unwrap();
        q.clear();
    }

    /// 与 [`parse_legacy_output`] 一致：首行即可判定失败并得到 `error_code`。
    fn fake_read_fail() -> &'static str {
        "退出码：1\n"
    }

    #[test]
    fn augment_empty_when_disabled() {
        reset_ring();
        let mut c = test_cfg();
        c.agent_tool_stats_enabled = false;
        c.thinking_avoid_echo_system_prompt = false;
        for _ in 0..5 {
            record_tool_outcome(&c, "read_file", fake_read_fail(), None, None);
        }
        let base = "BASE";
        assert_eq!(augment_system_prompt(base, &c), base);
    }

    #[test]
    fn thinking_appendix_when_tool_stats_off() {
        reset_ring();
        let mut c = test_cfg();
        c.agent_tool_stats_enabled = false;
        c.thinking_avoid_echo_system_prompt = true;
        let out = augment_system_prompt("P", &c);
        assert!(out.starts_with("P"));
        assert!(out.contains("思考过程纪律"));
    }

    #[test]
    fn augment_contains_tool_after_failures() {
        reset_ring();
        let c = test_cfg();
        for _ in 0..3 {
            record_tool_outcome(&c, "read_file", fake_read_fail(), None, None);
        }
        let out = augment_system_prompt("SYS", &c);
        assert!(out.contains("SYS"));
        assert!(out.contains("read_file"));
        assert!(out.contains("read_file_failed"));
    }

    #[test]
    fn low_success_ratio_triggers_line() {
        reset_ring();
        let mut c = test_cfg();
        c.agent_tool_stats_warn_below_success_ratio = 0.95;
        for _ in 0..3 {
            record_tool_outcome(&c, "run_command", "执行失败\n", None, None);
        }
        let out = augment_system_prompt("S", &c);
        assert!(out.contains("run_command"));
        assert!(out.contains("失败"));
    }
}
