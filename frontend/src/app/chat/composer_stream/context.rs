//! 单次 `/chat/stream` 回调共享的只读/句柄上下文（与 `callbacks` 分离，便于单测与浏览）。
//!
//! # 会话绑定
//!
//! [`ChatStreamCallbackCtx::bound_stream_session_id`] 为 **发起 attach 时** 的快照（见 [`super::make_attach_chat_stream`]），并与 [`crate::chat_session_state::ChatSessionSignals::stream_bound_session_id`] 同步写入。
//! [`ChatStreamCallbackCtx::attach_generation`] 与 [`crate::chat_session_state::ChatSessionSignals::stream_attach_generation`] 在发起时对齐，供各 `on_*` 丢弃陈旧回调。
//! 流式过程中即使用户切换 UI 的「当前会话」，SSE 仍应把增量写回**该场会话**在 `sessions` 中的那条记录；
//! 读写收口见 [`super::callbacks::stream_session_access`]。
//!
//! # 可变草稿
//!
//! [`ChatStreamCallbackCtx::scratch`] 承载本轮 attach 的可变草稿（[`super::stream_sse_scratch::StreamSseScratch`] → [`super::stream_turn_scratch_state::StreamTurnScratchState`]），与 Leptos `RwSignal` 解耦。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;

use super::super::handles::ComposerStreamShell;
use super::stream_sse_scratch::StreamSseScratch;

/// 各 `Rc<dyn Fn>` 共享：避免在闭包树中重复 `Arc::clone` 同一组字段。
pub(super) struct ChatStreamCallbackCtx {
    pub(super) chat: ChatSessionSignals,
    pub(super) locale: RwSignal<Locale>,
    pub(super) bound_stream_session_id: String,
    /// 与 [`ChatSessionSignals::stream_attach_generation`] 在发起 attach 时对齐；不一致表示本轮 SSE 已过期。
    pub(super) attach_generation: u64,
    pub(super) scratch: StreamSseScratch,
    pub(super) approval_session_store_id: String,
    pub(super) shell: ComposerStreamShell,
}

impl ChatStreamCallbackCtx {
    /// 当前闭包是否属于已过期的 attach（例如新一轮发送已 `abort` 上一轮但仍可能排队执行）。
    #[inline]
    pub(super) fn is_stale(&self) -> bool {
        self.chat.stream_attach_generation.get_untracked() != self.attach_generation
    }
}
