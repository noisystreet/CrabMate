//! 用户主动中止进行中的 **`/chat/stream`**：将 **`AbortController`**、壳层 **`status_busy` / `tool_busy`**
//! 与会话内 **assistant / 工具** 的 **`Loading`** 占位收口到 **[`apply_user_abort_of_inflight_stream`]**，
//! 避免接线层散落「只清信号、不改消息」的隐式分裂。
//!
//! 会话目标与 SSE 写入一致：使用 [`crate::chat_session_state::ChatSessionSignals::effective_stream_message_session_id`]。

use leptos::prelude::Set;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::i18n::Locale;
use crate::storage::StoredMessage;

use super::composer_stream::user_cancel_in_flight_stream;
use super::handles::ComposerStreamShell;

/// 用户从 Web 主列点击「停止」时的**唯一**收口（`cancel_stream` 闭包仅调用此处）。
///
/// 1. 若有在途流：`abort` 并置取消标志（见 [`user_cancel_in_flight_stream`]）。
/// 2. 在 [`ChatSessionSignals::effective_stream_message_session_id`] 对应会话上收尾 `Loading` 占位。
/// 3. 回落 **`status_busy` / `tool_busy`**。
///
/// 返回是否在途并成功发起中止（无在途流时为 `false`，无副作用）。
#[must_use]
pub(crate) fn apply_user_abort_of_inflight_stream(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
    loc: Locale,
) -> bool {
    if !user_cancel_in_flight_stream(shell) {
        return false;
    }
    let sid = chat.effective_stream_message_session_id();
    finalize_loading_placeholders_after_user_abort_on_session(chat, &sid, loc);
    shell.stream.status_busy.set(false);
    shell.stream.tool_busy.set(false);
    true
}

fn finalize_loading_placeholders_after_user_abort_on_session(
    chat: ChatSessionSignals,
    session_id: &str,
    loc: Locale,
) {
    chat.update_sessions_composer(|list| {
        let Some(s) = list.iter_mut().find(|s| s.id == session_id) else {
            return;
        };
        apply_abort_finalization_to_messages(&mut s.messages, loc);
    });
}

fn apply_abort_finalization_to_messages(messages: &mut Vec<StoredMessage>, loc: Locale) {
    let running_label = i18n::status_tool_running(loc);
    let stopped_tool = i18n::status_tool_stopped_user(loc);
    if let Some(m) = messages.iter_mut().rev().find(|m| {
        m.role == "assistant" && !m.is_tool && m.state.as_ref().is_some_and(|st| st.is_loading())
    }) {
        m.state = None;
        if m.text.trim().is_empty() {
            m.text = i18n::stream_stopped_inline(loc).to_string();
        } else {
            m.text.push_str(i18n::stream_stopped_suffix(loc));
        }
    }
    for m in messages.iter_mut() {
        if !m.is_tool || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
            continue;
        }
        m.state = None;
        if m.reasoning_text.contains("status: running") {
            m.reasoning_text = m
                .reasoning_text
                .replace("status: running", "status: stopped (user)");
        }
        if m.text.contains(running_label) {
            m.text = m.text.replacen(running_label, stopped_tool, 1);
        } else if m.text.trim().is_empty() {
            m.text = i18n::stream_stopped_inline(loc).to_string();
        } else {
            m.text.push_str(i18n::stream_stopped_suffix(loc));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_abort_finalization_to_messages;
    use crate::i18n::Locale;
    use crate::storage::{StoredMessage, StoredMessageState};

    fn loading_tool(text: &str) -> StoredMessage {
        StoredMessage {
            id: "t1".to_string(),
            role: "system".to_string(),
            text: text.to_string(),
            reasoning_text: "tool: x\nstatus: running".to_string(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("git".to_string()),
            created_at: 0,
        }
    }

    #[test]
    fn abort_clears_tool_loading_and_replaces_running_detail() {
        let mut msgs = vec![loading_tool("摘要 · 工具执行中…")];
        apply_abort_finalization_to_messages(&mut msgs, Locale::ZhHans);
        let m = &msgs[0];
        assert!(!m.state.as_ref().is_some_and(|s| s.is_loading()));
        assert!(m.reasoning_text.contains("stopped"));
        assert!(m.text.contains("已终止") || m.text.contains("Stopped"));
    }
}
