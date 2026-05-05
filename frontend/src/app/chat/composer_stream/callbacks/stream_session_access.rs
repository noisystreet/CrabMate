//! 流式 SSE 回调内对「当前 `ChatStreamCallbackCtx::active_session_id` 会话」的读写收口，
//! 统一 `find(|s| s.id == aid)`，避免 `builders` / `helpers` / `assemble` 各处重复拼条件。

use leptos::prelude::*;

use crate::storage::ChatSession;

use super::super::context::ChatStreamCallbackCtx;

pub(super) fn with_active_session_mut(
    stream_ctx: &ChatStreamCallbackCtx,
    f: impl FnOnce(&mut ChatSession),
) {
    let aid = stream_ctx.active_session_id.as_str();
    stream_ctx.chat.sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
            f(s);
        }
    });
}

pub(super) fn with_active_session_ref<R>(
    stream_ctx: &ChatStreamCallbackCtx,
    f: impl FnOnce(&ChatSession) -> R,
) -> Option<R> {
    let aid = stream_ctx.active_session_id.as_str();
    stream_ctx
        .chat
        .sessions
        .with(|list| list.iter().find(|s| s.id == aid).map(f))
}
