//! 尾部「非工具助手」loading 状态：供消息行打字点等 UI **单次**订阅，避免每行 `sessions.with` 扫全表。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;

/// 当前活动会话里，自末尾起第一条非工具助手消息若仍为 `loading`，返回其 `message_id`。
#[must_use]
pub(crate) fn tail_loading_assistant_mid_memo(chat: ChatSessionSignals) -> Memo<Option<String>> {
    let sessions = chat.sessions;
    let active_id = chat.active_id;
    Memo::new(move |_| {
        let aid = active_id.get();
        sessions.with(|list| {
            let session = list.iter().find(|s| s.id == aid)?;
            let tail = session
                .messages
                .iter()
                .rev()
                .find(|m| !m.is_tool && m.role == "assistant")?;
            if tail.state.as_ref().is_some_and(|s| s.is_loading()) {
                Some(tail.id.clone())
            } else {
                None
            }
        })
    })
}
