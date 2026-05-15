//! 角色正文管道的共用片段（剥前缀、用户过滤、层级子目标裁剪）。

use crate::i18n::Locale;
use crate::message_format::display::plan_fence::assistant_text_for_display;
use crate::message_format::staged_timeline::STAGED_TIMELINE_SYSTEM_PREFIX;
/// 须与主仓 `src/runtime/plan_section.rs` 中 `STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX` 同步。
pub(crate) const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";

/// 与 `src/agent/agent_turn/staged/sse.rs`、`plan_optimizer.rs`、`plan_ensemble.rs` 等注入的 **system** 首行对齐；聊天区展示时剥除，避免「分阶段 ·」原样进气泡。
pub(super) const STAGED_PLAN_SYSTEM_COACH_PREFIX: &str = "### 分阶段规划 ·";

fn coach_ordinal_from_injection_header(first_line: &str) -> usize {
    let s = first_line.trim();
    if s.contains("步骤优化") {
        2
    } else if s.contains("逻辑规划员") || s.contains("合并多规划") || s.contains("追加规划员")
    {
        3
    } else {
        1
    }
}

fn text_has_leading_ordinal_prefix(s: &str) -> bool {
    let t = s.trim_start();
    let mut end = 0usize;
    for (i, c) in t.char_indices() {
        if c.is_ascii_digit() {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return false;
    }
    t[end..].trim_start().starts_with('.')
}

/// 剥除 `### 分阶段规划 · …` 首行；返回 `(展示用序号, 剩余正文)`；不匹配则 `None`。
fn strip_staged_plan_system_coach_header(body_after_timeline: &str) -> Option<(usize, &str)> {
    let trimmed = body_after_timeline.trim_start();
    if !trimmed.starts_with(STAGED_PLAN_SYSTEM_COACH_PREFIX) {
        return None;
    }
    let first_nl = trimmed.find('\n').unwrap_or(trimmed.len());
    let first_line = trimmed[..first_nl].trim();
    let ord = coach_ordinal_from_injection_header(first_line);
    let rest = if first_nl < trimmed.len() {
        &trimmed[first_nl + 1..]
    } else {
        ""
    };
    Some((ord, rest))
}

/// 时间线或 `timeline_log` 若误把整段 `agent_reply_plan` JSON 当作正文，按助手管道转成可读列表，避免气泡回显原始 JSON。
fn maybe_format_timeline_standalone_agent_reply_plan(body: &str, loc: Locale) -> String {
    let t = body.trim();
    if !t.starts_with('{') {
        return body.to_string();
    }
    if !(t.contains("\"agent_reply_plan\"")
        || (t.contains("\"type\"") && t.contains("\"steps\"") && t.contains("\"version\"")))
    {
        return body.to_string();
    }
    let formatted = assistant_text_for_display(t, false, loc, true);
    let f = formatted.trim();
    if f.starts_with('{') && (f.contains("agent_reply_plan") || f.contains("\"type\"")) {
        return body.to_string();
    }
    if f.is_empty() {
        return body.to_string();
    }
    formatted
}

pub(super) fn system_text_for_chat_display(raw: &str, loc: Locale) -> String {
    let after_timeline = raw
        .strip_prefix(STAGED_TIMELINE_SYSTEM_PREFIX)
        .unwrap_or(raw);
    let after_timeline = maybe_format_timeline_standalone_agent_reply_plan(after_timeline, loc);
    if let Some((ord, rest)) = strip_staged_plan_system_coach_header(&after_timeline) {
        let rest = rest.trim_start();
        if rest.is_empty() {
            let hint = crate::i18n::staged_coach_injection_fallback(loc, ord);
            return format!("{ord}. {hint}");
        }
        if text_has_leading_ordinal_prefix(rest) {
            return rest.to_string();
        }
        return format!("{ord}. {rest}");
    }
    after_timeline.to_string()
}

pub(super) fn user_text_for_chat_display(raw: &str) -> String {
    let t = raw.trim_start();
    // 过滤 NL 补全桥接消息
    if t.starts_with(STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX) {
        return String::new();
    }
    // 过滤分步注入的 user 消息（与后端 is_staged_step_injection_user_content 一致）
    if t.contains("\n- id:")
        && t.contains("\n- 描述:")
        && (t.starts_with("### 分步 ") || t.starts_with("【分步执行"))
    {
        return String::new();
    }
    raw.to_string()
}

pub(super) fn maybe_trim_hierarchical_subgoal_redundant_lines(
    state: Option<&crate::storage::StoredMessageState>,
    raw: String,
    apply_assistant_display_filters: bool,
) -> String {
    if !apply_assistant_display_filters {
        return raw;
    }
    let is_subgoal = state.is_some_and(|s| s.looks_like_hierarchical_subgoal());
    if !is_subgoal {
        return raw;
    }
    let kept = raw
        .lines()
        .filter(|line| {
            let t = line.trim();
            !(t.starts_with("子目标 `")
                || t.starts_with("子目标 goal_")
                || t.starts_with("- 阶段：")
                || t.starts_with("阶段："))
        })
        .collect::<Vec<_>>()
        .join("\n");
    kept.trim().to_string()
}
