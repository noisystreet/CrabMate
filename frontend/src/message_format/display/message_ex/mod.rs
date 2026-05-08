//! 按角色拼出气泡展示用正文（`message_text_for_display_ex`）。

mod assistant;
mod parts;

use crate::i18n::Locale;
use crate::storage::StoredMessage;

#[cfg(test)]
pub(crate) use parts::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;

use assistant::assistant_message_text_for_display_ex;
use parts::{system_text_for_chat_display, user_text_for_chat_display};

/// `apply_assistant_display_filters == false` 时助手消息按存储原文输出（不剥 `agent_reply_plan`、不拆内联思维链标记）。
pub fn message_text_for_display_ex(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
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
