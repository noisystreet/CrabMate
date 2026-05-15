//! `/chat/stream` SSE 回调装配：与输入框 / 发送按钮解耦。
//!
//! - [`context`]：单次流式共享的 `ChatStreamCallbackCtx`。
//! - [`per_stream_accum`]：单轮流内的 `Cell`/`RefCell` 累计（正文增量计数、结束 reason 等），与 ctx 分层。
//! - [`stream_turn_scratch_state`]：单轮流 **lane + 尾泡 + FIFO** 收进 **单一 `RefCell` 快照**（与 `stream_turn_state` 枚举语义对齐）。
//! - [`stream_sse_scratch`]：[`StreamSseScratch`] 句柄，委托 [`stream_turn_scratch_state::StreamTurnScratchState`]。
//! - [`stream_turn_state`]：模型输出车道 [`StreamModelOutputLane`] 及 `lane_*` 的 `Cell` 薄封装（单测与热路径）。
//! - `shell_abort`：`AbortController` 与用户取消 Mutex 的集中读写。
//! - [`callbacks`]：装配 `ChatStreamCallbacks`（各 `on_*`），与 `send_chat_stream` 契约对齐；实现拆为 `callbacks/helpers`、`callbacks/builders`、`callbacks/assemble`。
//! - [`stream_attach_lifecycle`]：单次 attach 在 `spawn_local` 前的同步步骤（[`prepare_stream_attach`]、[`stream_attach_lifecycle::StreamAttachPrepared`]）。
//! - 壳层 **`status_busy` / `tool_busy`** 迁移见 **[`crate::app::stream_shell_busy`]**（[`crate::app::stream_shell_busy::StreamShellBusyOp`] + [`crate::app::app_signals::StreamControlSignals::apply_busy_op`] / [`crate::app::app_signals::StreamControlSignals::apply_release_turn_and_stream_run`]），SSE/HTTP/中止路径统一调用。
//! - 整轮 HTTP+SSE **运行相**见 **[`crate::app::stream_run_phase`]**（[`crate::app::app_signals::StreamControlSignals::begin_stream_run`] / [`crate::app::app_signals::StreamControlSignals::end_stream_run_if_current`]），与 `attach_generation` 门闩对齐。

mod callbacks;
mod context;
mod per_stream_accum;
mod shell_abort;
mod stream_attach_lifecycle;
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

use super::handles::ComposerStreamShell;
use shell_abort::user_cancelled_flag;
use stream_attach_lifecycle::prepare_stream_attach;

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
            let prepared = prepare_stream_attach(chat, &shell_outer, locale_sig, asst_id.clone());
            let agent_role = selected_agent_role.get();
            let cbs = callbacks::build_chat_stream_callbacks(Rc::clone(&prepared.stream_ctx));

            let gen_snapshot = prepared.attach_generation;
            let shell_for_stream_err = shell_outer.clone();
            let on_error_spawn = cbs.on_error.clone();
            let appr = prepared.approval_session_id.clone();
            spawn_local(async move {
                let stream_result = send_chat_stream(SendChatStreamParams {
                    message: user_text,
                    image_urls,
                    conversation_id: conv,
                    agent_role,
                    approval_session_id: Some(appr),
                    stream_resume_job_id: None,
                    stream_resume_after_seq: None,
                    signal: &prepared.abort_signal,
                    cbs: cbs.clone(),
                    loc: locale_sig.get_untracked(),
                    clarify_questionnaire_answers: clarify_json,
                })
                .await;
                if chat.stream_attach_generation_untracked() != gen_snapshot {
                    return;
                }
                // HTTP 读取结束后强制回落壳层 busy，避免悬挂连接或回调提前 return 时状态栏卡死。
                // 会话内 Loading 工具占位仅在 `on_error` / 用户中止等路径于 `messages` 上收口；本层不改时间线。
                // 正常路径仍由 `on_done` / `on_stream_ended` / `on_error` 与上述收口协同清理。
                shell_for_stream_err
                    .stream
                    .apply_release_turn_and_stream_run(gen_snapshot);
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

pub(crate) use shell_abort::{clear_abort_slot, user_cancel_in_flight_stream};
