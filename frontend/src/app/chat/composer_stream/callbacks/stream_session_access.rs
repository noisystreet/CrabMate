//! 流式 SSE 回调内对 **attach 时绑定的写入会话**（[`ChatStreamCallbackCtx::bound_stream_session_id`](super::super::context::ChatStreamCallbackCtx::bound_stream_session_id)）的读写收口，
//! 统一 `find(|s| s.id == sid)`，避免 `builders` / `helpers` / `assemble` 各处重复拼条件。
//! 热路径 **[`append_stream_assistant_chunk`]** 为 assistant 正文/思维链增量的**唯一**写入口（经 [`crate::chat_session_state::ChatSessionSignals::update_sessions_stream_sse`]）。
//! **调试构建**下校验 [`crate::chat_session_state::ChatSessionSignals::stream_bound_session_id`] 与 `sid` 一致（若已设置）。
//! 与 [`super::super::per_stream_accum::PerStreamAccum`] 分工：此处只碰 `sessions` 向量；累计计数在 `PerStreamAccum`。

use leptos::prelude::*;

use crate::storage::ChatSession;

use super::super::context::ChatStreamCallbackCtx;

#[cfg(debug_assertions)]
fn debug_assert_sse_session_binding(stream_ctx: &ChatStreamCallbackCtx, sid: &str) {
    if let Some(ref bound) = stream_ctx.chat.stream_bound_session_id.get() {
        debug_assert_eq!(
            bound.as_str(),
            sid,
            "stream_bound_session_id 应与 ChatStreamCallbackCtx.bound_stream_session_id 一致"
        );
    }
}

/// SSE `on_delta`：向本轮 attach 绑定会话中正在生成的 assistant 气泡追加文本或思维链。
pub(super) fn append_stream_assistant_chunk(
    stream_ctx: &ChatStreamCallbackCtx,
    message_id: &str,
    chunk: &str,
    to_reasoning: bool,
) {
    with_stream_write_session_mut(stream_ctx, |s| {
        if let Some(m) = s.messages.iter_mut().find(|m| m.id == message_id) {
            if to_reasoning {
                m.reasoning_text.push_str(chunk);
            } else {
                m.text.push_str(chunk);
            }
        }
    });
}

/// 对 **本轮 SSE 应写入的会话**（[`ChatStreamCallbackCtx::bound_stream_session_id`]，未必等于当前 UI `active_id`）做可变访问。
pub(super) fn with_stream_write_session_mut(
    stream_ctx: &ChatStreamCallbackCtx,
    f: impl FnOnce(&mut ChatSession),
) {
    let sid = stream_ctx.bound_stream_session_id.as_str();
    #[cfg(debug_assertions)]
    debug_assert_sse_session_binding(stream_ctx, sid);
    stream_ctx.chat.update_sessions_stream_sse(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == sid) {
            f(s);
        }
    });
}

/// 对本轮 SSE 绑定会话做只读访问。
pub(super) fn with_stream_write_session_ref<R>(
    stream_ctx: &ChatStreamCallbackCtx,
    f: impl FnOnce(&ChatSession) -> R,
) -> Option<R> {
    let sid = stream_ctx.bound_stream_session_id.as_str();
    #[cfg(debug_assertions)]
    debug_assert_sse_session_binding(stream_ctx, sid);
    stream_ctx
        .chat
        .sessions
        .with(|list| list.iter().find(|s| s.id == sid).map(f))
}
