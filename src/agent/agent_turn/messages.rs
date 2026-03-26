//! 助手消息合并与「本轮用户后」UI 分隔线插入。

use crate::types::{Message, is_chat_ui_separator};

pub(crate) fn push_assistant_merging_trailing_empty_placeholder(
    messages: &mut Vec<Message>,
    msg: Message,
) {
    if msg.role != "assistant" {
        messages.push(msg);
        return;
    }
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
        && last.tool_calls.is_none()
        && last
            .content
            .as_deref()
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        *last = msg;
        return;
    }
    messages.push(msg);
}

pub(crate) fn insert_separator_after_last_user_for_turn(messages: &mut Vec<Message>) {
    let Some(user_idx) = messages.iter().rposition(|m| m.role == "user") else {
        return;
    };
    if messages.get(user_idx + 1).is_some_and(is_chat_ui_separator) {
        return;
    }
    let sep = Message::chat_ui_separator(true);
    match messages.get(user_idx + 1) {
        Some(m) if m.role == "assistant" => {
            messages.insert(user_idx + 1, sep);
        }
        _ => {
            messages.push(sep);
        }
    }
}
