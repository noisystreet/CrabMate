//! 按角色拼出气泡展示用正文（`message_text_for_display_ex`）。

use std::borrow::Cow;

use crate::i18n::Locale;
use crate::storage::StoredMessage;

use super::super::staged_timeline::STAGED_TIMELINE_SYSTEM_PREFIX;
use super::plan_fence::assistant_text_for_display;
use super::thinking_strip::{
    assistant_thinking_body_and_answer_raw, filter_assistant_thinking_markers_for_display,
};

/// 须与主仓 `src/runtime/plan_section.rs` 中 `STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX` 同步。
pub(crate) const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";

/// 与 `src/agent/agent_turn/staged_sse.rs`、`plan_optimizer.rs`、`plan_ensemble.rs` 等注入的 **system** 首行对齐；聊天区展示时剥除，避免「分阶段 ·」原样进气泡。
const STAGED_PLAN_SYSTEM_COACH_PREFIX: &str = "### 分阶段规划 ·";

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

fn system_text_for_chat_display(raw: &str, loc: Locale) -> String {
    let after_timeline = raw
        .strip_prefix(STAGED_TIMELINE_SYSTEM_PREFIX)
        .unwrap_or(raw);
    if let Some((ord, rest)) = strip_staged_plan_system_coach_header(after_timeline) {
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

fn user_text_for_chat_display(raw: &str) -> String {
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

fn maybe_trim_hierarchical_subgoal_redundant_lines(
    state: Option<&str>,
    raw: String,
    apply_assistant_display_filters: bool,
) -> String {
    if !apply_assistant_display_filters {
        return raw;
    }
    let is_subgoal = state.is_some_and(|s| s.starts_with("hierarchical-subgoal:"));
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

/// `apply_assistant_display_filters == false` 时助手消息按存储原文输出（不剥 `agent_reply_plan`、不拆内联思维链标记）。
pub fn message_text_for_display_ex(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if m.role == "assistant" {
        let is_streaming_last_assistant = m.state.as_deref() == Some("loading");
        let reasoning_for_split: Cow<str> = if apply_assistant_display_filters {
            Cow::Owned(filter_assistant_thinking_markers_for_display(
                m.reasoning_text.as_str(),
                is_streaming_last_assistant,
            ))
        } else {
            Cow::Borrowed(m.reasoning_text.as_str())
        };
        let text_for_split: Cow<str> = if apply_assistant_display_filters {
            Cow::Owned(filter_assistant_thinking_markers_for_display(
                m.text.as_str(),
                is_streaming_last_assistant,
            ))
        } else {
            Cow::Borrowed(m.text.as_str())
        };
        let (r_body, t_body) = assistant_thinking_body_and_answer_raw(
            reasoning_for_split.as_ref(),
            text_for_split.as_ref(),
            apply_assistant_display_filters,
        );
        let answer = assistant_text_for_display(
            t_body,
            is_streaming_last_assistant,
            loc,
            apply_assistant_display_filters,
        );
        if apply_assistant_display_filters {
            let r = r_body.trim();
            let a = answer.trim();
            let t_trim = t_body.trim();
            let text_looks_like_plan_json = t_trim.contains("\"agent_reply_plan\"")
                || t_trim.contains("\"type\":\"agent_reply_plan\"");
            let answer_is_plan_digest_only = text_looks_like_plan_json
                && (a.is_empty() || a == crate::i18n::plan_generated(loc));
            let out = if r.is_empty() {
                answer
            } else if a.is_empty() || answer_is_plan_digest_only {
                // Web SSE：`assistant_answer_phase` 之前的增量写入 `reasoning_text`，之后写入 `text`。
                // 分阶段无工具规划轮（尤其 `no_task`）可能从未进入终答相，导致「规划前言 + ```json` 围栏」
                // 全在 `reasoning_text` 而 `text` 为空（或仅剥出「已生成分阶段规划。」占位）；若此处仅回显 `r`，
                // 会跳过对围栏内 `agent_reply_plan` 的剥离。
                let merged = format!("{}\n\n{}", r_body.trim_end(), t_body.trim_start());
                let merged_out =
                    assistant_text_for_display(&merged, is_streaming_last_assistant, loc, true);
                let mv = merged_out.trim();
                if mv.is_empty() || mv != r {
                    merged_out
                } else {
                    r.to_string()
                }
            } else {
                format!("{r}\n\n{answer}")
            };
            maybe_trim_hierarchical_subgoal_redundant_lines(
                m.state.as_deref(),
                out,
                apply_assistant_display_filters,
            )
        } else {
            let r_empty = r_body.trim().is_empty();
            let a_empty = answer.trim().is_empty();
            let out = if r_empty {
                answer
            } else if a_empty {
                r_body.to_string()
            } else {
                format!("{r_body}\n\n{answer}")
            };
            maybe_trim_hierarchical_subgoal_redundant_lines(
                m.state.as_deref(),
                out,
                apply_assistant_display_filters,
            )
        }
    } else if m.role == "user" {
        user_text_for_chat_display(&m.text)
    } else if m.role == "system" {
        system_text_for_chat_display(m.text.as_str(), loc)
    } else {
        m.text.clone()
    }
}
