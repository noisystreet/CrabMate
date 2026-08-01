//! 时间线旁注气泡推送；**布局转移**见 [`super::super::turn_layout::TurnLayout`]。

use leptos::prelude::GetUntracked;

use crate::session_ops::{make_message_id, message_created_ms};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::stream_text_overlay::stream_overlay_merged_text_reasoning_owned;

use super::super::turn_layout::TurnLayout;
use crate::app::chat::composer_stream::context::ChatStreamCallbackCtx;
use crate::message_dedupe::assistant_texts_fuzzy_duplicate;

pub(crate) fn push_assistant_timeline_bubble(
    stream_ctx: &ChatStreamCallbackCtx,
    text: String,
    state: Option<StoredMessageState>,
) {
    if text.trim().is_empty() {
        return;
    }
    let msg = StoredMessage {
        id: make_message_id(),
        role: "assistant".to_string(),
        text,
        reasoning_text: String::new(),
        image_urls: vec![],
        state,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: message_created_ms(),
    };
    TurnLayout::push_assistant_timeline(stream_ctx, msg);
}

pub(crate) fn assistant_message_has_visible_text(
    stream_ctx: &ChatStreamCallbackCtx,
    text: &str,
) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    stream_ctx
        .read_bound_session(|s| {
            s.messages.iter().any(|m| {
                m.role == "assistant"
                    && !m.is_tool
                    && (m.text.trim() == needle
                        || assistant_texts_fuzzy_duplicate(m.text.as_str(), needle))
            })
        })
        .unwrap_or(false)
}

pub(crate) fn streaming_assistant_tail_has_text(
    stream_ctx: &ChatStreamCallbackCtx,
    text: &str,
) -> bool {
    let needle = text.trim();
    if needle.is_empty() {
        return false;
    }
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let overlay = stream_ctx.chat.stream_text_overlay.get_untracked();
    stream_ctx
        .read_bound_session(|s| {
            s.messages.iter().any(|m| {
                if m.id != mid || m.role != "assistant" || m.is_tool {
                    return false;
                }
                let visible =
                    stream_overlay_merged_text_reasoning_owned(m, overlay.as_ref(), sid.as_str())
                        .map(|(t, _)| t)
                        .unwrap_or_else(|| m.text.clone());
                visible.trim() == needle
                    || assistant_texts_fuzzy_duplicate(visible.as_str(), needle)
            })
        })
        .unwrap_or(false)
}
