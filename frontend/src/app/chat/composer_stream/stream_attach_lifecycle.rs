//! 单次 `/chat/stream` attach 的**同步准备**：代际 bump、会话绑定、[`ChatStreamCallbackCtx`] 与中止控制器登记。
//!
//! 将 [`super::make_attach_chat_stream`] 闭包中 `spawn_local` 之前的步骤收拢为单入口，便于对照
//! [`ChatStreamCallbackCtx::attach_generation`] 门闩与 [`crate::chat_session_state::ChatStreamTransport`]。

use std::rc::Rc;

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::approval_session_id;

use crate::app::chat::composer_stream::context::ChatStreamCallbackCtx;
use crate::app::chat::composer_stream::shell_abort::{
    reset_abort_state_for_new_attach, store_abort_controller,
};
use crate::app::chat::composer_stream::stream_sse_scratch::StreamSseScratch;
use crate::app::chat::handles::ComposerStreamShell;
use crate::app::chat::stream_user_abort::finalize_superseded_assistant_loading_rows_except;

/// [`prepare_stream_attach`] 的产物：`spawn_local` 内发起 HTTP/SSE 与代际校验共用。
pub(crate) struct StreamAttachPrepared {
    pub(crate) stream_ctx: Rc<ChatStreamCallbackCtx>,
    pub(crate) attach_generation: u64,
    /// 已登记到 [`crate::app::app_signals::StreamControlSignals::abort_cell`] 的控制器所暴露的信号。
    pub(crate) abort_signal: web_sys::AbortSignal,
    /// 与 [`ChatStreamCallbackCtx::approval_session_store_id`] 同源，须原样传入 `send_chat_stream`（不可再次调用 [`approval_session_id`](crate::session_ops::approval_session_id)）。
    pub(crate) approval_session_id: String,
}

/// 绑定本轮流式写入目标并构造回调上下文（**不含** `send_chat_stream` / `spawn_local`）。
pub(crate) fn prepare_stream_attach(
    chat: ChatSessionSignals,
    shell: &ComposerStreamShell,
    locale_sig: RwSignal<Locale>,
    asst_id: String,
) -> StreamAttachPrepared {
    chat.clear_stream_resume_handles();
    let attach_generation = chat.bump_stream_attach_generation();
    shell.approval.thinking_trace_log.set(Vec::new());
    reset_abort_state_for_new_attach(shell);
    let bound_session_id = chat.active_id.get();
    finalize_superseded_assistant_loading_rows_except(
        chat,
        bound_session_id.as_str(),
        asst_id.as_str(),
        locale_sig.get_untracked(),
    );
    chat.bind_stream_to_session(bound_session_id.clone(), attach_generation);
    let ac = web_sys::AbortController::new().expect("AbortController");
    let abort_signal = ac.signal();
    store_abort_controller(shell, ac);
    let appr = approval_session_id();
    let appr_store = appr.clone();

    let stream_ctx = Rc::new(ChatStreamCallbackCtx {
        chat,
        locale: locale_sig,
        bound_stream_session_id: bound_session_id,
        attach_generation,
        scratch: StreamSseScratch::new(asst_id),
        approval_session_store_id: appr_store,
        shell: shell.clone(),
    });

    StreamAttachPrepared {
        stream_ctx,
        attach_generation,
        abort_signal,
        approval_session_id: appr,
    }
}
