//! `/chat/stream` SSE 回调装配：与输入框 / 发送按钮解耦。
//!
//! - [`context`]：单次流式共享的 `ChatStreamCallbackCtx`。
//! - [`per_stream_accum`]：单轮流内的 `Cell`/`RefCell` 累计（正文增量计数、结束 reason 等），与 ctx 分层。
//! - [`stream_turn_scratch_state`]：单轮流 **lane + 尾泡 + FIFO** 收进 **单一 `RefCell` 快照**（与 `stream_turn_state` 枚举语义对齐）。
//! - [`stream_sse_scratch`]：[`StreamSseScratch`] 句柄，委托 [`stream_turn_scratch_state::StreamTurnScratchState`]。
//! - [`stream_turn_state`]：模型输出车道 [`StreamModelOutputLane`] 及 `lane_*` 的 `Cell` 薄封装（单测与热路径）。
//! - `shell_abort`：`AbortController` 与用户取消 Mutex 的集中读写。
//! - [`callbacks`]：装配 `ChatStreamCallbacks`（各 `on_*`），与 `send_chat_stream` 契约对齐；实现拆为 `callbacks/helpers`、`callbacks/builders`、`callbacks/assemble`。
//! - 本文件：长生命周期句柄 [`ComposerStreamHandles`]、[`make_attach_chat_stream`]（发起请求 + `spawn_local`）。

mod callbacks;
mod context;
mod per_stream_accum;
mod shell_abort;
mod stream_sse_scratch;
mod stream_turn_scratch_state;
mod stream_turn_state;

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{SendChatStreamParams, send_chat_stream};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::approval_session_id;

use super::handles::ComposerStreamShell;
use super::stream_user_abort::finalize_superseded_assistant_loading_rows_except;

use context::ChatStreamCallbackCtx;
use shell_abort::{reset_abort_state_for_new_attach, store_abort_controller, user_cancelled_flag};
use stream_sse_scratch::StreamSseScratch;

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
            let attach_generation = chat.bump_stream_attach_generation();
            shell_outer.approval.thinking_trace_log.set(Vec::new());
            reset_abort_state_for_new_attach(&shell_outer);
            let bound_session_id = chat.active_id.get();
            finalize_superseded_assistant_loading_rows_except(
                chat,
                bound_session_id.as_str(),
                asst_id.as_str(),
                locale_sig.get_untracked(),
            );
            chat.bind_stream_to_session(bound_session_id.clone());
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            store_abort_controller(&shell_outer, ac);
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();

            let stream_ctx = Rc::new(ChatStreamCallbackCtx {
                chat,
                locale: locale_sig,
                bound_stream_session_id: bound_session_id,
                attach_generation,
                scratch: StreamSseScratch::new(asst_id.clone()),
                approval_session_store_id: appr_store.clone(),
                shell: shell_outer.clone(),
            });

            let cbs = callbacks::build_chat_stream_callbacks(Rc::clone(&stream_ctx));

            let gen_snapshot = attach_generation;
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
                if chat.stream_attach_generation.get_untracked() != gen_snapshot {
                    return;
                }
                // HTTP 读取结束后必须回落 busy：正常路径已由 `on_done` / `on_stream_ended` / `on_error` 清理；
                // 若连接悬挂、取消分支提前 return、或回调遗漏，避免状态栏永久「模型生成中」。
                shell_for_stream_err.stream.status_busy.set(false);
                shell_for_stream_err.stream.tool_busy.set(false);
                if let Err(e) = stream_result {
                    if user_cancelled_flag(&shell_for_stream_err) {
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
