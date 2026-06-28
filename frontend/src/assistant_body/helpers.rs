//! 从会话列表解析当前助手气泡展示数据，供 [`super::view`] 与 `Effect` 共用，避免三处重复 `find` 逻辑。

use leptos::prelude::*;

use crate::i18n::Locale;
use crate::storage::{ChatSession, StoredMessage};
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

/// 超过该字符数的已完成助手消息可手动折叠（作用于整条消息，含思考区）。
pub(super) const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

/// 单条助手消息在 UI 上用于上色 / 折叠判断的快照（已由 `message_format` 做过滤与拼接）。
#[derive(Clone, PartialEq)]
pub(super) struct AssistantMsgSnapshot {
    pub(super) display_text: String,
    pub(super) is_loading: bool,
    pub(super) display_char_len: usize,
}

/// 仅当本条消息仍 `loading` 或 overlay 正指向它时才订阅 [`StreamTextOverlay`] 热路径；
/// 否则用 `get_untracked`，避免第二轮起每个 token 触发**全部**历史助手气泡重算 Markdown（主线程假卡死）。
#[must_use]
pub(super) fn should_track_stream_overlay_for_message(
    msg: &StoredMessage,
    overlay: Option<&StreamTextOverlay>,
    session_id: &str,
) -> bool {
    if msg.state.as_ref().is_some_and(|s| s.is_loading()) {
        return true;
    }
    overlay.is_some_and(|o| o.session_id == session_id && o.message_id == msg.id)
}

/// 在活动会话中按 `message_id` 查找助手消息并生成展示快照。
pub(super) fn snapshot_assistant_message_for_mid(
    sessions: &[ChatSession],
    active_session_id: &str,
    message_id: &str,
    locale: Locale,
    apply_assistant_display_filters: bool,
    stream_overlay: Option<&StreamTextOverlay>,
) -> Option<AssistantMsgSnapshot> {
    let msg = sessions
        .iter()
        .find(|s| s.id == active_session_id)?
        .messages
        .iter()
        .find(|m| m.id == message_id)?;
    let display_text = message_text_for_display_including_stream_overlay(
        msg,
        stream_overlay,
        active_session_id,
        locale,
        apply_assistant_display_filters,
    );
    let is_loading = msg.state.as_ref().is_some_and(|s| s.is_loading());
    let display_char_len = display_text.chars().count();
    Some(AssistantMsgSnapshot {
        display_text,
        is_loading,
        display_char_len,
    })
}

/// 单条助手 Markdown 气泡：合并 overlay 后的展示快照，供 `Effect` 与 `class=` / `Show` **共享同一 Memo**，减少重复 `sessions.with`。
#[must_use]
pub(super) fn assistant_markdown_display_memo(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    message_id: String,
    stream_text_overlay: RwSignal<Option<StreamTextOverlay>>,
    stream_overlay_display_mid: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> Memo<Option<AssistantMsgSnapshot>> {
    let mid = message_id;
    Memo::new(move |_| {
        let aid = active_id.get();
        let loc = locale.get();
        let apply = apply_assistant_display_filters.get();

        let track_overlay = match stream_overlay_display_mid.get().as_deref() {
            Some(active) => active == mid.as_str(),
            None => sessions.with(|list| {
                list.iter()
                    .find(|s| s.id == aid)
                    .and_then(|s| s.messages.iter().find(|m| m.id == mid))
                    .is_some_and(|msg| {
                        should_track_stream_overlay_for_message(
                            msg,
                            stream_text_overlay.get_untracked().as_ref(),
                            aid.as_str(),
                        )
                    })
            }),
        };

        let ov = if track_overlay {
            stream_text_overlay.get()
        } else {
            stream_text_overlay.get_untracked()
        };

        sessions.with(|list| {
            snapshot_assistant_message_for_mid(
                list,
                aid.as_str(),
                mid.as_str(),
                loc,
                apply,
                ov.as_ref(),
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn plain_asst(id: &str, loading: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: "hi".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: loading.then_some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn track_overlay_when_loading_or_targeted() {
        let msg = plain_asst("a1", true);
        assert!(should_track_stream_overlay_for_message(&msg, None, "s1"));
        let done = plain_asst("a1", false);
        let ov = StreamTextOverlay {
            session_id: "s1".into(),
            message_id: "a1".into(),
            answer: "x".into(),
            reasoning: String::new(),
        };
        assert!(should_track_stream_overlay_for_message(
            &done,
            Some(&ov),
            "s1"
        ));
        assert!(!should_track_stream_overlay_for_message(
            &done,
            Some(&ov),
            "s2"
        ));
        assert!(!should_track_stream_overlay_for_message(
            &plain_asst("other", false),
            Some(&ov),
            "s1"
        ));
    }
}
