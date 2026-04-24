//! `/chat/stream` SSE 回调装配：与输入框 / 发送按钮解耦。
//!
//! - [`context`]：单次流式共享的 `ChatStreamCallbackCtx`。
//! - [`callbacks`]：装配 `ChatStreamCallbacks`（各 `on_*`），与 `send_chat_stream` 契约对齐。
//! - 本文件：长生命周期句柄 [`ComposerStreamHandles`]、[`make_attach_chat_stream`]（发起请求 + `spawn_local`）。

mod callbacks;
mod context;

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::VecDeque;

use crate::api::send_chat_stream;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::approval_session_id;

use super::handles::ComposerStreamShell;

use context::ChatStreamCallbackCtx;

/// 长生命周期句柄：`attach` 闭包捕获，供每次发起流式请求复用。
pub(super) struct ComposerStreamHandles {
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub shell: ComposerStreamShell,
}

type AttachChatStreamFn =
    dyn Fn(String, Vec<String>, String, Option<serde_json::Value>) + Send + Sync;

pub(super) fn make_attach_chat_stream(h: ComposerStreamHandles) -> Arc<AttachChatStreamFn> {
    let ComposerStreamHandles {
        chat,
        locale: locale_sig,
        selected_agent_role,
        shell,
    } = h;

    Arc::new({
        let shell_outer = shell.clone();
        let chat = chat;
        let locale_sig = locale_sig;
        let selected_agent_role = selected_agent_role;
        move |user_text: String,
              image_urls: Vec<String>,
              asst_id: String,
              clarify_json: Option<serde_json::Value>| {
            let conv = chat.session_sync.with(|s| s.stream_conversation_id());
            chat.stream_job_id.set(None);
            chat.stream_last_event_seq.set(0);
            shell_outer.thinking_trace_log.set(Vec::new());
            if let Some(prev) = shell_outer.abort_cell.lock().unwrap().take() {
                prev.abort();
            }
            *shell_outer.user_cancelled_stream.lock().unwrap() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *shell_outer.abort_cell.lock().unwrap() = Some(ac);
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Arc::clone(&shell_outer.user_cancelled_stream);

            let stream_ctx = Rc::new(ChatStreamCallbackCtx {
                chat,
                locale: locale_sig,
                active_session_id: chat.active_id.get(),
                assistant_message_id: asst_id.clone(),
                approval_session_store_id: appr_store.clone(),
                shell: shell_outer.clone(),
                pending_tool_args: Rc::new(RefCell::new(context::PendingToolArgs::default())),
                pending_tool_message_ids: Rc::new(RefCell::new(VecDeque::new())),
            });

            let in_answer_phase: Rc<Cell<bool>> = Rc::new(Cell::new(false));
            let cbs = callbacks::build_chat_stream_callbacks(stream_ctx, in_answer_phase);

            let shell_for_stream_err = shell_outer.clone();
            let on_error_spawn = cbs.on_error.clone();
            spawn_local(async move {
                let stream_result = send_chat_stream(
                    user_text,
                    image_urls,
                    conv,
                    agent_role,
                    Some(appr_for_stream),
                    None,
                    None,
                    &signal,
                    cbs.clone(),
                    locale_sig.get_untracked(),
                    clarify_json,
                )
                .await;
                if let Err(e) = stream_result {
                    if *user_cancelled_for_spawn.lock().unwrap() {
                        return;
                    }
                    if e == "stream stopped" {
                        return;
                    }
                    shell_for_stream_err.status_err.set(Some(e.clone()));
                    on_error_spawn(e);
                }
            });
        }
    })
}
