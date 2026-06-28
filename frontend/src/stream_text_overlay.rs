//! 流式助手正文/思维链的 **旁路缓冲**：SSE `on_delta` 只更新本信号，不触碰 [`crate::chat_session_state::ChatSessionSignals::sessions`]，
//! 避免长会话下每条历史消息随 token 反复参与 Leptos 追踪与 `<For>` 重算。
//!
//! # 展示层「单一真源」读取
//!
//! 对**任意**可能处于 `loading` 的助手气泡，凡需与用户所见一致的字符串（侧栏全局搜索、会话内查找、复制、Markdown 快照等），
//! 应调用 [`message_text_for_display_including_stream_overlay`]，并把 `parent_session_id` 设为**承载该消息的会话 id**
//!（即 [`crate::storage::ChatSession::id`]；跨会话遍历时每条的父会话 id，**不必**等于 UI 当前 `active_id`）。
//! 勿仅对 `StoredMessage` 调 [`crate::message_format::message_text_for_display_ex`]，否则会漏掉仍仅在 overlay 中的尾段正文。
//!
//! 在收尾路径（`on_done` / `on_error` / 工具前后轮换 / 用户中止等）经 [`stream_overlay_take_into_stored_message`]
//! 合并回 `StoredMessage` 并清空缓冲；[`sessions_snapshot_with_stream_overlay_merged`] 供持久化防抖落盘时与内存一致。

use leptos::prelude::*;

use crate::i18n::Locale;
use crate::message_format::{
    assistant_message_text_for_display_ex_with_body_strings, message_text_for_display_ex,
};
use crate::storage::{ChatSession, StoredMessage};

/// 当前 attach 内、尾条 `loading` 助手消息的流式增量（与 `sessions` 中的该条 id 对齐）。
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct StreamTextOverlay {
    pub session_id: String,
    pub message_id: String,
    pub answer: String,
    pub reasoning: String,
}

/// SSE 热路径：仅 bump `stream_text_overlay`，**不** `sessions.update`。
pub fn stream_overlay_append(
    overlay: RwSignal<Option<StreamTextOverlay>>,
    session_id: &str,
    message_id: &str,
    chunk: &str,
    to_reasoning: bool,
    revision: Option<RwSignal<u64>>,
) {
    overlay.update(|opt| {
        let mut next = match opt.take() {
            Some(o) if o.session_id == session_id && o.message_id == message_id => o,
            Some(_) | None => StreamTextOverlay {
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
                answer: String::new(),
                reasoning: String::new(),
            },
        };
        if to_reasoning {
            next.reasoning.push_str(chunk);
        } else {
            next.answer.push_str(chunk);
        }
        *opt = Some(next);
    });
    if let Some(rev) = revision {
        rev.update(|n| *n = n.wrapping_add(1));
    }
}

/// 将缓冲合并进 `msg`（仅当 `session_id` / `message_id` 一致），并清空 overlay。
pub fn stream_overlay_take_into_stored_message(
    overlay: RwSignal<Option<StreamTextOverlay>>,
    session_id: &str,
    message_id: &str,
    msg: &mut StoredMessage,
) {
    overlay.update(|opt| {
        let taken = opt.take();
        let Some(o) = taken else {
            return;
        };
        if o.session_id == session_id && o.message_id == message_id {
            msg.text.push_str(&o.answer);
            msg.reasoning_text.push_str(&o.reasoning);
        } else {
            *opt = Some(o);
        }
    });
}

/// 若 `overlay` 命中本条助手消息（`session_id` + `message_id` 对齐），返回合并后的 `text` / `reasoning_text`。
///
/// 不限于 `loading`：`final_response` / 工具前轮换等会提前去掉 `Loading`，但同 attach 内 delta 仍写入 overlay，
/// 若此处要求 `is_loading()`，会出现「流式生成一段后 UI 不再更新」的假卡死。
#[must_use]
pub fn stream_overlay_merged_text_reasoning_owned(
    msg: &StoredMessage,
    overlay: Option<&StreamTextOverlay>,
    parent_session_id: &str,
) -> Option<(String, String)> {
    let o = overlay?;
    if o.session_id != parent_session_id || o.message_id != msg.id {
        return None;
    }
    if msg.role != "assistant" || msg.is_tool {
        return None;
    }
    let mut text = String::with_capacity(msg.text.len() + o.answer.len());
    text.push_str(&msg.text);
    text.push_str(&o.answer);
    let mut reasoning = String::with_capacity(msg.reasoning_text.len() + o.reasoning.len());
    reasoning.push_str(&msg.reasoning_text);
    reasoning.push_str(&o.reasoning);
    Some((text, reasoning))
}

/// 与 [`message_text_for_display_ex`] 一致，但合并当前流式 overlay（若适用）。
///
/// `parent_session_id`：本条 `m` 所属会话的 id（与 [`StreamTextOverlay::session_id`] 对齐时才会合并 overlay）。
#[must_use]
pub fn message_text_for_display_including_stream_overlay(
    m: &StoredMessage,
    overlay: Option<&StreamTextOverlay>,
    parent_session_id: &str,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if m.role == "assistant" {
        if let Some((text, reasoning)) =
            stream_overlay_merged_text_reasoning_owned(m, overlay, parent_session_id)
        {
            return assistant_message_text_for_display_ex_with_body_strings(
                text.as_str(),
                reasoning.as_str(),
                m.state.as_ref(),
                locale,
                apply_assistant_display_filters,
            );
        }
    }
    message_text_for_display_ex(m, locale, apply_assistant_display_filters)
}

/// 持久化前把 overlay 合并进克隆列表，避免落盘缺尾段。
#[must_use]
pub fn sessions_snapshot_with_stream_overlay_merged(
    sessions: &[ChatSession],
    overlay: Option<&StreamTextOverlay>,
) -> Vec<ChatSession> {
    let mut out = sessions.to_vec();
    let Some(o) = overlay else {
        return out;
    };
    let Some(s) = out.iter_mut().find(|session| session.id == o.session_id) else {
        return out;
    };
    let Some(m) = s.messages.iter_mut().find(|msg| msg.id == o.message_id) else {
        return out;
    };
    if m.role == "assistant" && !m.is_tool {
        m.text.push_str(&o.answer);
        m.reasoning_text.push_str(&o.reasoning);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

    #[test]
    fn append_then_take_merges_into_message() {
        let overlay = RwSignal::new(None::<StreamTextOverlay>);
        stream_overlay_append(overlay, "s1", "m1", "hello", false, None);
        stream_overlay_append(overlay, "s1", "m1", " world", false, None);
        let mut msg = StoredMessage {
            id: "m1".into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        stream_overlay_take_into_stored_message(overlay, "s1", "m1", &mut msg);
        assert_eq!(msg.text, "hello world");
        assert!(overlay.get().is_none());
    }

    #[test]
    fn merged_text_reasoning_matches_push_str_semantics() {
        let msg = StoredMessage {
            id: "m1".into(),
            role: "assistant".into(),
            text: "base ".into(),
            reasoning_text: "r0 ".into(),
            image_urls: vec!["/u/x.png".into()],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let o = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "m1".into(),
            answer: "tail".into(),
            reasoning: "r1".into(),
        };
        let (t, r) = stream_overlay_merged_text_reasoning_owned(&msg, Some(&o), "s1")
            .expect("overlay should apply");
        assert_eq!(t, "base tail");
        assert_eq!(r, "r0 r1");
    }

    #[test]
    fn merged_overlay_applies_after_loading_cleared() {
        let msg = StoredMessage {
            id: "m1".into(),
            role: "assistant".into(),
            text: "stored ".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        let o = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "m1".into(),
            answer: "tail".into(),
            reasoning: String::new(),
        };
        let (t, r) = stream_overlay_merged_text_reasoning_owned(&msg, Some(&o), "s1")
            .expect("overlay should merge after loading cleared");
        assert_eq!(t, "stored tail");
        assert!(r.is_empty());
    }

    #[test]
    fn persist_snapshot_merges_overlay_without_loading() {
        let session = ChatSession {
            id: "s1".into(),
            title: "t".into(),
            draft: String::new(),
            messages: vec![StoredMessage {
                id: "m1".into(),
                role: "assistant".into(),
                text: "base".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            }],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let o = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "m1".into(),
            answer: "+delta".into(),
            reasoning: String::new(),
        };
        let merged =
            sessions_snapshot_with_stream_overlay_merged(std::slice::from_ref(&session), Some(&o));
        assert_eq!(merged[0].messages[0].text, "base+delta");
    }

    #[test]
    fn persist_snapshot_merges_overlay() {
        let session = ChatSession {
            id: "s1".into(),
            title: "t".into(),
            draft: String::new(),
            messages: vec![StoredMessage {
                id: "m1".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            }],
            updated_at: 0,
            pinned: false,
            starred: false,
            server_conversation_id: None,
            server_revision: None,
            workspace_root: None,
            history_total: None,
            history_window_start: None,
            history_has_older: None,
        };
        let o = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "m1".into(),
            answer: "x".into(),
            reasoning: String::new(),
        };
        let merged =
            sessions_snapshot_with_stream_overlay_merged(std::slice::from_ref(&session), Some(&o));
        assert_eq!(merged[0].messages[0].text, "x");
        assert_eq!(session.messages[0].text, "");
    }
}
