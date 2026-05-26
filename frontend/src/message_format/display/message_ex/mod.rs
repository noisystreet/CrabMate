//! 按角色拼出气泡展示用正文（`message_text_for_display_ex`）。

mod assistant;
mod parts;

use crate::i18n::Locale;
use crate::message_format::stored_tool_message_compact_text;
use crate::storage::{StoredMessage, StoredMessageState};

#[cfg(test)]
pub(crate) use crabmate_display_rules::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;

use assistant::{
    assistant_message_text_for_display_ex, assistant_message_text_for_display_ex_with_body,
};
use parts::{system_text_for_chat_display, user_text_for_chat_display};

/// `apply_assistant_display_filters == false` 时助手消息按存储原文输出（不剥 `agent_reply_plan`、不拆内联思维链标记）。
///
/// 仅反映已落盘到 [`StoredMessage`] 的正文；若助手仍处于 `loading` 且增量在 **SSE 旁路 overlay** 中，须改用
/// [`crate::stream_text_overlay::message_text_for_display_including_stream_overlay`]，否则会与气泡所见不一致。
pub fn message_text_for_display_ex(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if m.is_tool {
        return stored_tool_message_compact_text(m, loc);
    }
    if m.role == "assistant" {
        assistant_message_text_for_display_ex(m, loc, apply_assistant_display_filters)
    } else if m.role == "user" {
        user_text_for_chat_display(&m.text)
    } else if m.role == "system" {
        system_text_for_chat_display(m.text.as_str(), loc)
    } else {
        m.text.clone()
    }
}

/// 助手展示管道：与 [`assistant_message_text_for_display_ex`] 一致，但允许调用方传入合并后的正文/思维链（如 Web SSE overlay），避免克隆整条消息。
pub(crate) fn assistant_message_text_for_display_ex_with_body_strings(
    text: &str,
    reasoning_text: &str,
    state: Option<&StoredMessageState>,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    assistant_message_text_for_display_ex_with_body(
        text,
        reasoning_text,
        state,
        loc,
        apply_assistant_display_filters,
    )
}
