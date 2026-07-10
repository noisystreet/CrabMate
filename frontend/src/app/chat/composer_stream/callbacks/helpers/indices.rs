//! 消息向量查找：工具占位、`tool_call_id`、本地 `id` 命中。

use crate::message_loading::is_loading_tool_message;
use crate::storage::StoredMessage;

/// 按 OpenAI `tool_call_id` 查找仍处于 loading 的工具行（供 `tool_result` 命中占位气泡）。
#[must_use]
pub(crate) fn index_of_loading_tool_by_call_id(
    messages: &[StoredMessage],
    tool_call_id: &str,
) -> Option<usize> {
    let tid = tool_call_id.trim();
    if tid.is_empty() {
        return None;
    }
    messages
        .iter()
        .position(|m| m.tool_call_id.as_deref() == Some(tid) && is_loading_tool_message(m))
}

/// 按本地消息 `id` 查找行（FIFO 占位 id 与 `tool_result` 配对）。
#[must_use]
pub(crate) fn index_of_message_id(messages: &[StoredMessage], message_id: &str) -> Option<usize> {
    let mid = message_id.trim();
    if mid.is_empty() {
        return None;
    }
    messages.iter().position(|m| m.id == mid)
}

#[cfg(test)]
mod tool_call_index_tests {
    use super::{index_of_loading_tool_by_call_id, index_of_message_id};
    use crate::storage::{StoredMessage, StoredMessageState};

    fn tool_loading(id: &str, tool_call_id: Option<&str>) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: "system".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: tool_call_id.map(String::from),
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn index_by_tool_call_id() {
        let msgs = [tool_loading("a", None), tool_loading("b", Some("tid-1"))];
        assert_eq!(index_of_loading_tool_by_call_id(&msgs, "tid-1"), Some(1));
    }

    #[test]
    fn index_by_message_id() {
        let msgs = [tool_loading("x", None), tool_loading("y", None)];
        assert_eq!(index_of_message_id(&msgs, "y"), Some(1));
    }

    #[test]
    fn empty_tool_call_id_returns_none() {
        let msgs = [tool_loading("a", None)];
        assert_eq!(index_of_loading_tool_by_call_id(&msgs, "  "), None);
    }
}
