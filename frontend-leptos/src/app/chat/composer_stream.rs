//! `/chat/stream` SSE 回调装配：与输入框 / 发送按钮解耦，便于单独阅读与测试桩接。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{ChatStreamCallbacks, send_chat_stream};
use crate::chat_session_state::ChatSessionSignals;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::{self, Locale};
use crate::message_format::{staged_timeline_system_message_body, tool_card_text};
use crate::session_ops::{approval_session_id, make_message_id, message_created_ms};
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, ToolResultInfo,
};
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    timeline_state_staged_end, timeline_state_staged_start, timeline_state_tool,
};

/// 单次 `/chat/stream` 的 SSE 回调共享状态：各 `Rc<dyn Fn>` 只再包一层 `Rc<ChatStreamCallbackCtx>`，避免重复 `Arc::clone` 与多字段捕获。
pub(super) struct ChatStreamCallbackCtx {
    pub(super) chat: ChatSessionSignals,
    pub(super) locale: RwSignal<Locale>,
    pub(super) active_session_id: String,
    pub(super) assistant_message_id: String,
    pub(super) abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub(super) user_cancelled_stream: Arc<Mutex<bool>>,
    pub(super) status_busy: RwSignal<bool>,
    pub(super) status_err: RwSignal<Option<String>>,
    pub(super) tool_busy: RwSignal<bool>,
    pub(super) pending_approval: RwSignal<Option<(String, String, String)>>,
    pub(super) approval_session_store_id: String,
    pub(super) changelist_modal_open: RwSignal<bool>,
    pub(super) changelist_fetch_nonce: RwSignal<u64>,
    pub(super) refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub(super) pending_clarification: RwSignal<Option<PendingClarificationForm>>,
}

/// 长生命周期句柄：`attach` 闭包捕获，供每次发起流式请求复用。
pub(super) struct ComposerStreamHandles {
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub status_busy: RwSignal<bool>,
    pub status_err: RwSignal<Option<String>>,
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub tool_busy: RwSignal<bool>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
}

type AttachChatStreamFn =
    dyn Fn(String, Vec<String>, String, Option<serde_json::Value>) + Send + Sync;

pub(super) fn make_attach_chat_stream(h: ComposerStreamHandles) -> Arc<AttachChatStreamFn> {
    let ComposerStreamHandles {
        chat,
        locale: locale_sig,
        selected_agent_role,
        status_busy,
        status_err,
        pending_approval,
        tool_busy,
        abort_cell,
        user_cancelled_stream,
        refresh_workspace,
        changelist_modal_open,
        changelist_fetch_nonce,
        pending_clarification: pending_clarification_sig,
    } = h;

    Arc::new({
        let abort_cell = Arc::clone(&abort_cell);
        let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
        let chat = chat;
        let locale_sig = locale_sig;
        let selected_agent_role = selected_agent_role;
        let status_busy = status_busy;
        let status_err = status_err;
        let pending_approval = pending_approval;
        let tool_busy = tool_busy;
        let refresh_workspace = Arc::clone(&refresh_workspace);
        let changelist_modal_open = changelist_modal_open;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        let pending_clarification_sig = pending_clarification_sig;
        move |user_text: String,
              image_urls: Vec<String>,
              asst_id: String,
              clarify_json: Option<serde_json::Value>| {
            let conv = chat.session_sync.with(|s| s.stream_conversation_id());
            // 新一次 attach 必须**不带** `stream_resume`：断线重连仅由 `send_chat_stream` 内部循环
            // 用响应头里的 `x-stream-job-id` 与 `last_event_id` 完成。若此处读取 UI 上残留的
            // `stream_job_id`（例如上轮 SSE 报错未收到 `stream_ended`），会误用已 `remove_job` 的
            // id，首包即 410「无法重连」。
            chat.stream_job_id.set(None);
            chat.stream_last_event_seq.set(0);
            if let Some(prev) = abort_cell.lock().unwrap().take() {
                prev.abort();
            }
            *user_cancelled_stream.lock().unwrap() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *abort_cell.lock().unwrap() = Some(ac);
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Arc::clone(&user_cancelled_stream);

            let stream_ctx = Rc::new(ChatStreamCallbackCtx {
                chat,
                locale: locale_sig,
                active_session_id: chat.active_id.get(),
                assistant_message_id: asst_id.clone(),
                abort_cell: Arc::clone(&abort_cell),
                user_cancelled_stream: Arc::clone(&user_cancelled_stream),
                status_busy,
                status_err,
                tool_busy,
                pending_approval,
                approval_session_store_id: appr_store.clone(),
                changelist_modal_open,
                changelist_fetch_nonce,
                refresh_workspace: Arc::clone(&refresh_workspace),
                pending_clarification: pending_clarification_sig,
            });

            let in_answer_phase: Rc<Cell<bool>> = Rc::new(Cell::new(false));

            let on_delta: Rc<dyn Fn(String)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                let in_answer_phase = Rc::clone(&in_answer_phase);
                Rc::new(move |chunk: String| {
                    let aid = stream_ctx.active_session_id.as_str();
                    let mid = stream_ctx.assistant_message_id.as_str();
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                                if in_answer_phase.get() {
                                    m.text.push_str(&chunk);
                                } else {
                                    m.reasoning_text.push_str(&chunk);
                                }
                            }
                        }
                    });
                })
            };
            let on_done: Rc<dyn Fn()> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move || {
                    if *stream_ctx.user_cancelled_stream.lock().unwrap() {
                        *stream_ctx.abort_cell.lock().unwrap() = None;
                        return;
                    }
                    let loc = stream_ctx.locale.get_untracked();
                    let aid = stream_ctx.active_session_id.clone();
                    let mid = stream_ctx.assistant_message_id.clone();
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid)
                            && let Some(m) = s.messages.iter_mut().find(|m| m.id == mid)
                            && m.state.as_deref() == Some("loading")
                        {
                            // 仅收尾「仍在生成」的气泡；SSE 已 on_error 的勿覆盖 error 状态
                            m.state = None;
                            if m.text.trim().is_empty() && m.reasoning_text.trim().is_empty() {
                                m.text = i18n::stream_empty_reply(loc).to_string();
                            }
                        }
                    });
                    stream_ctx.status_busy.set(false);
                    *stream_ctx.abort_cell.lock().unwrap() = None;
                    // 流完全结束后再触发水合：此时助手气泡已退出 loading，且须在 `sessions.with` 之外
                    // 用 `session_hydrate_nonce` 单独驱动，避免「水合写回 messages → Effect 再拉取」的循环竞态。
                    stream_ctx
                        .chat
                        .session_hydrate_nonce
                        .update(|n| *n = n.wrapping_add(1));
                })
            };
            let on_error: Rc<dyn Fn(String)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |msg: String| {
                    if *stream_ctx.user_cancelled_stream.lock().unwrap() {
                        *stream_ctx.abort_cell.lock().unwrap() = None;
                        return;
                    }
                    stream_ctx.chat.stream_job_id.set(None);
                    stream_ctx.chat.stream_last_event_seq.set(0);
                    let aid = stream_ctx.active_session_id.clone();
                    let mid = stream_ctx.assistant_message_id.clone();
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == mid) {
                                m.text = msg;
                                m.state = Some("error".to_string());
                            }
                        }
                    });
                    stream_ctx.status_busy.set(false);
                    stream_ctx.status_err.set(Some(
                        i18n::chat_failed_banner(stream_ctx.locale.get_untracked()).to_string(),
                    ));
                    *stream_ctx.abort_cell.lock().unwrap() = None;
                })
            };
            let on_ws: Rc<dyn Fn()> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move || {
                    (stream_ctx.refresh_workspace)();
                    if stream_ctx.changelist_modal_open.get_untracked() {
                        stream_ctx
                            .changelist_fetch_nonce
                            .update(|x| *x = x.wrapping_add(1));
                    }
                })
            };
            let on_tool_status: Rc<dyn Fn(bool)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |b: bool| {
                    stream_ctx.tool_busy.set(b);
                })
            };
            let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |info: ToolResultInfo| {
                    let t = tool_card_text(&info, stream_ctx.locale.get_untracked());
                    let id = make_message_id();
                    let aid = stream_ctx.active_session_id.as_str();
                    let tl_ok = info.ok.unwrap_or(true);
                    let state = timeline_state_tool(&id, tl_ok);
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                            s.messages.push(StoredMessage {
                                id,
                                role: "system".to_string(),
                                text: t,
                                reasoning_text: String::new(),
                                image_urls: vec![],
                                state: Some(state),
                                is_tool: true,
                                created_at: message_created_ms(),
                            });
                        }
                    });
                })
            };
            let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |req: CommandApprovalRequest| {
                    stream_ctx.pending_approval.set(Some((
                        stream_ctx.approval_session_store_id.clone(),
                        req.command,
                        req.args,
                    )));
                })
            };
            let on_cid: Rc<dyn Fn(String)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |id: String| {
                    stream_ctx
                        .chat
                        .session_sync
                        .update(|s| s.apply_stream_conversation_id(id.clone()));
                    let aid = stream_ctx.active_session_id.clone();
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                            s.server_conversation_id = Some(id);
                            s.server_revision = None;
                        }
                    });
                    // 不在此处触发水合：SSE conversation_id 事件表示会话已在服务器端创建，
                    // 但消息尚未保存。水合应在 on_conversation_revision（收到 conversation_saved.revision）时触发。
                })
            };
            let on_conv_rev: Rc<dyn Fn(u64)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |rev: u64| {
                    stream_ctx
                        .chat
                        .session_sync
                        .update(|s| s.apply_saved_revision(rev));
                    let aid = stream_ctx.active_session_id.clone();
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                            s.server_revision = Some(rev);
                        }
                    });
                    // 水合由流结束时的 `on_done` 递增 `session_hydrate_nonce` 触发（见上），
                    // 勿在此处递增：否则在助手仍为 loading 时会与 `sessions` 订阅叠加，造成重复拉取与覆盖。
                })
            };
            let on_stream_ended: Rc<dyn Fn(String)> = {
                Rc::new(move |reason: String| {
                    if reason == "completed" || reason == "cancelled" {
                        chat.stream_job_id.set(None);
                        chat.stream_last_event_seq.set(0);
                    }
                })
            };
            let on_stream_job_id: Rc<dyn Fn(u64)> = {
                Rc::new(move |jid: u64| {
                    chat.stream_job_id.set(Some(jid));
                })
            };
            let on_last_sse_event_id: Rc<dyn Fn(u64)> = {
                Rc::new(move |seq: u64| {
                    chat.stream_last_event_seq.set(seq);
                })
            };
            let on_assistant_answer_phase: Rc<dyn Fn()> = {
                let in_answer_phase = Rc::clone(&in_answer_phase);
                Rc::new(move || in_answer_phase.set(true))
            };
            let on_staged_step_started: Rc<dyn Fn(StagedPlanStepStartInfo)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |info: StagedPlanStepStartInfo| {
                    let loc = stream_ctx.locale.get_untracked();
                    let text =
                        staged_timeline_system_message_body(&i18n::timeline_staged_step_started(
                            loc,
                            info.step_index,
                            info.total_steps,
                            &info.description,
                            info.executor_kind.as_deref(),
                        ));
                    let id = make_message_id();
                    let aid = stream_ctx.active_session_id.as_str();
                    let now = message_created_ms();
                    let state = timeline_state_staged_start(&id, info.step_index, info.total_steps);
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                            s.messages.push(StoredMessage {
                                id,
                                role: "system".to_string(),
                                text,
                                reasoning_text: String::new(),
                                image_urls: vec![],
                                state: Some(state),
                                is_tool: false,
                                created_at: now,
                            });
                        }
                    });
                })
            };
            let on_clarification: Rc<dyn Fn(ClarificationQuestionnaireInfo)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |info: ClarificationQuestionnaireInfo| {
                    stream_ctx
                        .pending_clarification
                        .set(Some(PendingClarificationForm::from_sse(info)));
                })
            };
            let on_staged_step_finished: Rc<dyn Fn(StagedPlanStepEndInfo)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |info: StagedPlanStepEndInfo| {
                    let loc = stream_ctx.locale.get_untracked();
                    let text =
                        staged_timeline_system_message_body(&i18n::timeline_staged_step_finished(
                            loc,
                            info.step_index,
                            info.total_steps,
                            &info.status,
                            info.executor_kind.as_deref(),
                        ));
                    let id = make_message_id();
                    let aid = stream_ctx.active_session_id.as_str();
                    let now = message_created_ms();
                    let state = timeline_state_staged_end(
                        &id,
                        info.step_index,
                        info.total_steps,
                        &info.status,
                    );
                    stream_ctx.chat.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                            s.messages.push(StoredMessage {
                                id,
                                role: "system".to_string(),
                                text,
                                reasoning_text: String::new(),
                                image_urls: vec![],
                                state: Some(state),
                                is_tool: false,
                                created_at: now,
                            });
                        }
                    });
                })
            };

            let cbs = ChatStreamCallbacks {
                on_delta,
                on_done: on_done.clone(),
                on_error: on_error.clone(),
                on_workspace_changed: on_ws,
                on_tool_status,
                on_tool_result,
                on_approval,
                on_conversation_id: on_cid,
                on_conversation_revision: on_conv_rev,
                on_stream_ended,
                on_stream_job_id,
                on_last_sse_event_id,
                on_assistant_answer_phase,
                on_staged_plan_step_started: on_staged_step_started,
                on_staged_plan_step_finished: on_staged_step_finished,
                on_clarification_questionnaire: on_clarification,
            };

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
                    // `stream stopped`：SSE 控制面已调用 `on_error`，勿再收尾以免覆盖助手气泡。
                    if e == "stream stopped" {
                        return;
                    }
                    status_err.set(Some(e.clone()));
                    on_error(e);
                }
            });
        }
    })
}
