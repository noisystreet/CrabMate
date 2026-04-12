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

fn user_text_for_chat_display(raw: &str) -> String {
    if raw
        .trim_start()
        .starts_with(STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX)
    {
        return String::new();
    }
    raw.to_string()
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
            if r.is_empty() {
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
            }
        } else {
            let r_empty = r_body.trim().is_empty();
            let a_empty = answer.trim().is_empty();
            if r_empty {
                answer
            } else if a_empty {
                r_body.to_string()
            } else {
                format!("{r_body}\n\n{answer}")
            }
        }
    } else if m.role == "user" {
        user_text_for_chat_display(&m.text)
    } else if m.role == "system" {
        m.text
            .strip_prefix(STAGED_TIMELINE_SYSTEM_PREFIX)
            .unwrap_or(m.text.as_str())
            .to_string()
    } else {
        m.text.clone()
    }
}
