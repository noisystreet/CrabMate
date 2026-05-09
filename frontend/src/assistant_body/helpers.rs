//! 从会话列表解析当前助手气泡展示数据，供 [`super::view`] 与 `Effect` 共用，避免三处重复 `find` 逻辑。

use crate::i18n::Locale;
use crate::message_format::message_text_for_display_ex;
use crate::storage::{ChatSession, StoredMessage};
use crate::stream_text_overlay::{StreamTextOverlay, stored_message_with_overlay_merged};

/// 超过该字符数的已完成助手消息可手动折叠（作用于整条消息，含思考区）。
pub(super) const LONG_ASSISTANT_COLLAPSE_THRESHOLD: usize = 2400;

/// 单条助手消息在 UI 上用于上色 / 折叠判断的快照（已由 `message_format` 做过滤与拼接）。
pub(super) struct AssistantMsgSnapshot {
    pub(super) display_text: String,
    pub(super) is_loading: bool,
    pub(super) display_char_len: usize,
}

fn snapshot_from_message(
    msg: &StoredMessage,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> AssistantMsgSnapshot {
    let display_text = message_text_for_display_ex(msg, locale, apply_assistant_display_filters);
    let is_loading = msg.state.as_ref().is_some_and(|s| s.is_loading());
    let display_char_len = display_text.chars().count();
    AssistantMsgSnapshot {
        display_text,
        is_loading,
        display_char_len,
    }
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
    let merged = stored_message_with_overlay_merged(msg, stream_overlay, active_session_id);
    Some(snapshot_from_message(
        &merged,
        locale,
        apply_assistant_display_filters,
    ))
}
