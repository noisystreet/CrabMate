//! `on_error` 中对会话 **`messages`** 的尾助手泡写回错误文案与状态，
//! 与 `with_active_session_mut` 解耦以便单测与降 [`super::builders::chat_stream_on_error_builder`] nloc。
//!
//! 流式错误路径应调用 [`apply_stream_error_on_messages`]，避免只改助手尾泡却遗漏仍 **`Loading`** 的工具时间线占位
//!（与 [`crate::app::chat::stream_user_abort`] 中用户中止收口对齐）。

use crate::app::chat::stream_user_abort::finalize_loading_tool_placeholders_to_stopped;
use crate::i18n::Locale;
use crate::storage::{StoredMessage, StoredMessageState};

/// 将 `assistant_message_id` 对应消息设为错误态并覆盖正文（若存在该 id）。
pub(super) fn apply_stream_error_to_assistant_message(
    messages: &mut Vec<StoredMessage>,
    assistant_message_id: &str,
    friendly_error: String,
) {
    if let Some(m) = messages.iter_mut().find(|m| m.id == assistant_message_id) {
        m.text = friendly_error;
        m.state = Some(StoredMessageState::Error);
    }
}

/// 流式 `on_error` 对绑定会话 **`messages`** 的完整写回：尾助手错误态 + 仍 **`Loading`** 的工具行收口。
pub(super) fn apply_stream_error_on_messages(
    messages: &mut Vec<StoredMessage>,
    assistant_message_id: &str,
    friendly_error: String,
    loc: Locale,
) {
    apply_stream_error_to_assistant_message(messages, assistant_message_id, friendly_error);
    finalize_loading_tool_placeholders_to_stopped(messages, loc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use crate::storage::{StoredMessage, StoredMessageState};

    fn msg(id: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: "assistant".to_string(),
            text: text.to_string(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn error_apply_updates_matching_id() {
        let mut messages = vec![msg("u1", "hi"), msg("a1", "…")];
        apply_stream_error_to_assistant_message(&mut messages, "a1", "boom".to_string());
        assert_eq!(messages[1].text, "boom");
        assert!(
            messages[1]
                .state
                .as_ref()
                .is_some_and(|s| matches!(s, StoredMessageState::Error))
        );
    }

    #[test]
    fn error_apply_noop_when_id_missing() {
        let mut messages = vec![msg("a1", "…")];
        apply_stream_error_to_assistant_message(&mut messages, "ghost", "x".to_string());
        assert_eq!(messages[0].text, "…");
        assert!(messages[0].state.is_none());
    }

    #[test]
    fn error_path_can_clear_loading_tool_after_assistant_error() {
        let mut messages = vec![
            msg("a1", "…"),
            StoredMessage {
                id: "t1".to_string(),
                role: "system".to_string(),
                text: "tool".to_string(),
                reasoning_text: "status: running".to_string(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: true,
                tool_call_id: None,
                tool_name: Some("x".to_string()),
                created_at: 0,
            },
        ];
        messages[0].state = Some(StoredMessageState::Loading);
        apply_stream_error_on_messages(&mut messages, "a1", "wall clock".to_string(), Locale::En);
        assert!(!messages[1].state.as_ref().is_some_and(|s| s.is_loading()));
    }
}
