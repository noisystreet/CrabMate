//! 单次 `/chat/stream` 回调共享的只读/句柄上下文（与 `callbacks` 分离，便于单测与浏览）。
//!
//! # 会话绑定
//!
//! [`ChatStreamCallbackCtx::active_session_id`] 为 **发起 attach 时** 的快照（见 [`super::make_attach_chat_stream`]），并与 [`crate::chat_session_state::ChatSessionSignals::stream_bound_session_id`] 同步写入。
//! 流式过程中即使用户切换 UI 的「当前会话」，SSE 仍应把增量写回**该场会话**在 `sessions` 中的那条记录；
//! 读写收口见 [`super::callbacks::stream_session_access`]。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;

use super::super::handles::ComposerStreamShell;
use super::streaming_tail::StreamingAssistantTail;

/// 各 `Rc<dyn Fn>` 共享：避免在闭包树中重复 `Arc::clone` 同一组字段。
pub(super) struct ChatStreamCallbackCtx {
    pub(super) chat: ChatSessionSignals,
    pub(super) locale: RwSignal<Locale>,
    pub(super) active_session_id: String,
    pub(super) tail: StreamingAssistantTail,
    pub(super) approval_session_store_id: String,
    pub(super) shell: ComposerStreamShell,
}
