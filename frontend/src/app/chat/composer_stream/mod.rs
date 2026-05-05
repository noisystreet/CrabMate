//! `/chat/stream` SSE 回调装配：与输入框 / 发送按钮解耦。
//!
//! - [`context`]：单次流式共享的 `ChatStreamCallbackCtx`。
//! - `shell_abort`：`AbortController` 与用户取消 Mutex 的集中读写。
//! - [`callbacks`]：装配 `ChatStreamCallbacks`（各 `on_*`），与 `send_chat_stream` 契约对齐；实现拆为 `callbacks/helpers`、`callbacks/builders`、`callbacks/assemble`。
//! - 本文件：长生命周期句柄 [`ComposerStreamHandles`]、[`make_attach_chat_stream`]（发起请求 + `spawn_local`）。

mod callbacks;
mod context;
mod shell_abort;

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use crate::api::{SendChatStreamParams, send_chat_stream};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::approval_session_id;

use super::handles::ComposerStreamShell;

use callbacks::new_stream_output_lane_cell;

use context::ChatStreamCallbackCtx;
use shell_abort::{reset_abort_state_for_new_attach, store_abort_controller};

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
            chat.clear_stream_resume_handles();
            shell_outer.approval.thinking_trace_log.set(Vec::new());
            reset_abort_state_for_new_attach(&shell_outer);
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            store_abort_controller(&shell_outer, ac);
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Arc::clone(&shell_outer.stream.user_cancelled_stream);

            let stream_ctx = Rc::new(ChatStreamCallbackCtx {
                chat,
                locale: locale_sig,
                active_session_id: chat.active_id.get(),
                assistant_message_id: RefCell::new(asst_id.clone()),
                post_tool_stream_tail: Cell::new(false),
                approval_session_store_id: appr_store.clone(),
                shell: shell_outer.clone(),
                pending_tool_message_ids: Rc::new(RefCell::new(VecDeque::new())),
            });

            let output_lane = new_stream_output_lane_cell();
            let cbs = callbacks::build_chat_stream_callbacks(stream_ctx, output_lane);

            let shell_for_stream_err = shell_outer.clone();
            let on_error_spawn = cbs.on_error.clone();
            spawn_local(async move {
                let stream_result = send_chat_stream(SendChatStreamParams {
                    message: user_text,
                    image_urls,
                    conversation_id: conv,
                    agent_role,
                    approval_session_id: Some(appr_for_stream),
                    stream_resume_job_id: None,
                    stream_resume_after_seq: None,
                    signal: &signal,
                    cbs: cbs.clone(),
                    loc: locale_sig.get_untracked(),
                    clarify_questionnaire_answers: clarify_json,
                })
                .await;
                // HTTP 读取结束后必须回落 busy：正常路径已由 `on_done` / `on_stream_ended` / `on_error` 清理；
                // 若连接悬挂、取消分支提前 return、或回调遗漏，避免状态栏永久「模型生成中」。
                shell_for_stream_err.stream.status_busy.set(false);
                shell_for_stream_err.stream.tool_busy.set(false);
                if let Err(e) = stream_result {
                    if *user_cancelled_for_spawn.lock().unwrap() {
                        return;
                    }
                    if e == "stream stopped" {
                        return;
                    }
                    shell_for_stream_err.stream.status_err.set(Some(e.clone()));
                    on_error_spawn(e);
                }
            });
        }
    })
}

pub(crate) use shell_abort::user_cancel_in_flight_stream;
