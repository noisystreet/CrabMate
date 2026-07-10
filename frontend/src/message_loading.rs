//! `/chat/stream` 与消息 UI 共用的 **Loading 占位谓词**（普通助手 / 工具行 / attach 冲突扫描）。

use crate::storage::{StoredMessage, StoredMessageState};

#[inline]
#[must_use]
pub fn stored_message_is_loading(m: &StoredMessage) -> bool {
    m.state.as_ref().is_some_and(StoredMessageState::is_loading)
}

#[inline]
#[must_use]
pub fn is_plain_assistant_message(m: &StoredMessage) -> bool {
    m.role == "assistant" && !m.is_tool
}

#[inline]
#[must_use]
pub fn is_loading_plain_assistant(m: &StoredMessage) -> bool {
    is_plain_assistant_message(m) && stored_message_is_loading(m)
}

#[inline]
#[must_use]
pub fn is_loading_tool_message(m: &StoredMessage) -> bool {
    m.is_tool && stored_message_is_loading(m)
}

/// 任意工具 Loading，或 **非** `except_plain_assistant_id` 的普通助手 Loading（截断再生 attach 门闩）。
#[must_use]
pub fn is_stream_attach_loading_conflict(
    m: &StoredMessage,
    except_plain_assistant_id: &str,
) -> bool {
    if !stored_message_is_loading(m) {
        return false;
    }
    if m.is_tool {
        return true;
    }
    is_plain_assistant_message(m) && m.id != except_plain_assistant_id
}

#[must_use]
pub fn messages_have_any_loading(messages: &[StoredMessage]) -> bool {
    messages.iter().any(stored_message_is_loading)
}

#[must_use]
pub fn messages_have_loading_tool(messages: &[StoredMessage]) -> bool {
    messages.iter().any(is_loading_tool_message)
}

#[must_use]
pub fn last_plain_assistant(messages: &[StoredMessage]) -> Option<&StoredMessage> {
    messages
        .iter()
        .rev()
        .find(|m| is_plain_assistant_message(m))
}

#[must_use]
pub fn tail_loading_plain_assistant_id(messages: &[StoredMessage]) -> Option<String> {
    last_plain_assistant(messages)
        .filter(|m| stored_message_is_loading(m))
        .map(|m| m.id.clone())
}

#[must_use]
pub fn is_loading_streaming_assistant_id(m: &StoredMessage, streaming_assistant_id: &str) -> bool {
    m.id == streaming_assistant_id && is_loading_plain_assistant(m)
}

/// post-tool 尾泡已去掉 `Loading` 但仍为普通助手行（peel / 过早 finalize 判定）。
#[must_use]
pub fn is_finalized_plain_assistant(m: &StoredMessage) -> bool {
    is_plain_assistant_message(m) && !stored_message_is_loading(m)
}

/// 消息行 CSS「loading」：普通助手或工具时间线行且 state 为 Loading。
#[inline]
#[must_use]
pub fn message_row_shows_loading(
    is_tool: bool,
    role: &str,
    state: Option<&StoredMessageState>,
) -> bool {
    state.is_some_and(StoredMessageState::is_loading) && (role == "assistant" || is_tool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn msg(role: &str, is_tool: bool, state: Option<StoredMessageState>) -> StoredMessage {
        StoredMessage {
            id: "m1".into(),
            role: role.into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state,
            is_tool,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn plain_assistant_loading_and_conflict_except() {
        let loading = msg("assistant", false, Some(StoredMessageState::Loading));
        assert!(is_loading_plain_assistant(&loading));
        assert!(!is_stream_attach_loading_conflict(&loading, "m1"));
        assert!(is_stream_attach_loading_conflict(&loading, "other"));
    }

    #[test]
    fn tool_loading_conflicts_regardless_of_except() {
        let tool = msg("system", true, Some(StoredMessageState::Loading));
        assert!(is_loading_tool_message(&tool));
        assert!(is_stream_attach_loading_conflict(&tool, "any"));
    }

    #[test]
    fn tail_loading_plain_assistant_from_rev_scan() {
        let messages = vec![msg("assistant", false, None), {
            let mut tail = msg("assistant", false, Some(StoredMessageState::Loading));
            tail.id = "tail".into();
            tail
        }];
        assert_eq!(
            tail_loading_plain_assistant_id(&messages).as_deref(),
            Some("tail")
        );
    }
}
