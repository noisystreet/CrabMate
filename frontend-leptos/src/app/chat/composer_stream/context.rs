//! 单次 `/chat/stream` 回调共享的只读/句柄上下文（与 `callbacks` 分离，便于单测与浏览）。

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;

use super::super::handles::ComposerStreamShell;

/// 暂存最近一次 tool_call 的参数信息。
#[derive(Debug, Clone, Default)]
pub(super) struct PendingToolArgs {
    pub(super) preview: Option<String>,
    pub(super) full: Option<String>,
}

/// 各 `Rc<dyn Fn>` 共享：避免在闭包树中重复 `Arc::clone` 同一组字段。
pub(super) struct ChatStreamCallbackCtx {
    pub(super) chat: ChatSessionSignals,
    pub(super) locale: RwSignal<Locale>,
    pub(super) active_session_id: String,
    pub(super) assistant_message_id: String,
    pub(super) approval_session_store_id: String,
    pub(super) shell: ComposerStreamShell,
    /// 暂存最近一次 tool_call 的参数。
    pub(super) pending_tool_args: Rc<RefCell<PendingToolArgs>>,
}
