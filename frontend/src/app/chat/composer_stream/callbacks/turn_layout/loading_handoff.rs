//! 工具前旁白投影后的 loading 所有权移交。

use leptos::prelude::GetUntracked;

use crate::stream_text_overlay::{
    stream_overlay_answer_for_message, stream_overlay_clear_answer_for_message,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::bubble_queue::is_commentary_row_id;

/// 旁注行已投影后：将 live loading 的所有权移交给 commentary 行。
///
/// 须在 `sync_turn_projection` 已 flush 出与 **当前 live 正文同文** 的
/// `turn-commentary-*` 之后调用。不得仅因会话里「任意」旁注行存在就清空——
/// 多轮时旧轮 commentary 仍在，会把本轮尚未落盘的旁白掏空。
pub(super) fn release_loading_preview_after_commentary_projected(
    stream_ctx: &ChatStreamCallbackCtx,
) {
    let mid = stream_ctx.scratch.clone_assistant_id();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let overlay = stream_overlay_answer_for_message(
        stream_ctx.chat.stream_text_overlay.get_untracked().as_ref(),
        sid.as_str(),
        mid.as_str(),
    );
    let stored_text = stream_ctx
        .read_bound_session(|s| {
            s.messages
                .iter()
                .find(|m| m.id == mid.as_str())
                .map(|m| m.text.clone())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let live = if !stored_text.trim().is_empty() {
        stored_text
    } else {
        overlay.unwrap_or_default()
    };
    let live_trim = live.trim();
    if live_trim.is_empty() {
        return;
    }
    let handed_off = stream_ctx
        .read_bound_session(|s| {
            s.messages
                .iter()
                .any(|m| is_commentary_row_id(m.id.as_str()) && m.text.trim() == live_trim)
        })
        .unwrap_or(false);
    if !handed_off {
        return;
    }
    stream_ctx.update_bound_session(|s| {
        if let Some(idx) = s.messages.iter().position(|m| m.id == mid.as_str()) {
            s.messages[idx].text.clear();
        }
    });
    stream_overlay_clear_answer_for_message(
        stream_ctx.chat.stream_text_overlay,
        sid.as_str(),
        mid.as_str(),
        Some(stream_ctx.chat.stream_overlay_revision),
    );
}
