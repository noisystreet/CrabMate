//! 输入区与流式对话：草稿缓冲、发送 / 停止、重试 / 截断再生、新会话。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::api::{ChatStreamCallbacks, send_chat_stream};
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::{self, Locale};
use crate::message_format::{staged_timeline_system_message_body, tool_card_text};
use crate::session_ops::{
    approval_session_id, flush_composer_draft_to_session, make_message_id, message_created_ms,
    patch_active_session, prepare_retry_failed_assistant_turn, title_from_user_prompt,
};
use crate::session_sync::SessionSyncState;
use crate::sse_dispatch::{
    ClarificationQuestionnaireInfo, CommandApprovalRequest, StagedPlanStepEndInfo,
    StagedPlanStepStartInfo, ToolResultInfo,
};
use crate::storage::{ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, make_session_id};
use crate::timeline_scan::{
    timeline_state_staged_end, timeline_state_staged_start, timeline_state_tool,
};

/// 单次 `/chat/stream` 的 SSE 回调共享状态：各 `Rc<dyn Fn>` 只再包一层 `Rc<ChatStreamCallbackCtx>`，避免重复 `Arc::clone` 与多字段捕获。
struct ChatStreamCallbackCtx {
    sessions: RwSignal<Vec<ChatSession>>,
    locale: RwSignal<Locale>,
    active_session_id: String,
    assistant_message_id: String,
    abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    user_cancelled_stream: Arc<Mutex<bool>>,
    status_busy: RwSignal<bool>,
    status_err: RwSignal<Option<String>>,
    tool_busy: RwSignal<bool>,
    pending_approval: RwSignal<Option<(String, String, String)>>,
    approval_session_store_id: String,
    session_sync: RwSignal<SessionSyncState>,
    session_hydrate_nonce: RwSignal<u64>,
    changelist_modal_open: RwSignal<bool>,
    changelist_fetch_nonce: RwSignal<u64>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
}

pub(super) struct ChatComposerWires {
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub cancel_stream: Arc<dyn Fn() + Send + Sync>,
    pub new_session: Rc<dyn Fn()>,
}

/// 切换会话时重置会话级 UI 状态并加载该会话草稿（勿订阅 `sessions`，避免流式更新覆盖缓冲）。
#[allow(clippy::too_many_arguments)]
pub(super) fn wire_session_switch_clears_chat_state(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    session_sync: RwSignal<SessionSyncState>,
    stream_job_id: RwSignal<Option<u64>>,
    stream_last_event_seq: RwSignal<u64>,
    expanded_long_assistant_ids: RwSignal<Vec<String>>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
) {
    Effect::new(move |_| {
        let id = active_id.get();
        if !initialized.get() {
            return;
        }
        let list = sessions.get_untracked();
        let d = list
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        draft.set(d);
        pending_images.set(Vec::new());
        pending_clarification.set(None);
        // 仅用上方 `list`（get_untracked）：勿再 `sessions.with`，否则 effect 会订阅流式
        // `sessions` 更新，每次落盘消息都会清空多选勾选。
        let st = list.iter().find(|s| s.id == id).map(|s| {
            let mut st = SessionSyncState::local_only();
            if let Some(ref cid) = s.server_conversation_id {
                let t = cid.trim();
                if !t.is_empty() {
                    st.apply_stream_conversation_id(t.to_string());
                    if let Some(rev) = s.server_revision {
                        st.apply_saved_revision(rev);
                    }
                }
            }
            st
        });
        session_sync.set(st.unwrap_or_else(SessionSyncState::local_only));
        stream_job_id.set(None);
        stream_last_event_seq.set(0);
        expanded_long_assistant_ids.set(Vec::new());
        bubble_md_selected_ids.set(Vec::new());
    });
}

/// `draft` 程序化更新时同步 Mutex 与 textarea（输入过程不订阅 `draft`）。
pub(super) fn wire_draft_sync_to_buffer_and_textarea(
    draft: RwSignal<String>,
    composer_draft_buffer: Arc<Mutex<String>>,
    composer_input_ref: NodeRef<Textarea>,
) {
    Effect::new({
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let composer_input_ref = composer_input_ref.clone();
        move |_| {
            let d = draft.get();
            *composer_draft_buffer.lock().unwrap() = d.clone();
            let d_for_dom = d.clone();
            let cref = composer_input_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = cref.get() {
                    if el.value() != d_for_dom {
                        el.set_value(&d_for_dom);
                    }
                }
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn wire_chat_composer_streams(
    initialized: RwSignal<bool>,
    sessions: RwSignal<Vec<ChatSession>>,
    locale: RwSignal<Locale>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    session_hydrate_nonce: RwSignal<u64>,
    session_sync: RwSignal<SessionSyncState>,
    stream_job_id: RwSignal<Option<u64>>,
    stream_last_event_seq: RwSignal<u64>,
    selected_agent_role: RwSignal<Option<String>>,
    status_busy: RwSignal<bool>,
    status_err: RwSignal<Option<String>>,
    pending_approval: RwSignal<Option<(String, String, String)>>,
    tool_busy: RwSignal<bool>,
    composer_draft_buffer: Arc<Mutex<String>>,
    auto_scroll_chat: RwSignal<bool>,
    abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    user_cancelled_stream: Arc<Mutex<bool>>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    changelist_modal_open: RwSignal<bool>,
    changelist_fetch_nonce: RwSignal<u64>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
) -> ChatComposerWires {
    type AttachChatStreamFn =
        dyn Fn(String, Vec<String>, String, Option<serde_json::Value>) + Send + Sync;
    let attach_chat_stream: Arc<AttachChatStreamFn> = Arc::new({
        let abort_cell = Arc::clone(&abort_cell);
        let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
        let sessions = sessions;
        let locale_sig = locale;
        let active_id = active_id;
        let session_hydrate_nonce = session_hydrate_nonce;
        let session_sync = session_sync;
        let stream_job_id_sig = stream_job_id;
        let stream_last_event_seq_sig = stream_last_event_seq;
        let selected_agent_role = selected_agent_role;
        let status_busy = status_busy;
        let status_err = status_err;
        let pending_approval = pending_approval;
        let tool_busy = tool_busy;
        let refresh_workspace = Arc::clone(&refresh_workspace);
        let changelist_modal_open = changelist_modal_open;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        let pending_clarification_sig = pending_clarification;
        move |user_text: String,
              image_urls: Vec<String>,
              asst_id: String,
              clarify_json: Option<serde_json::Value>| {
            let conv = session_sync.with(|s| s.stream_conversation_id());
            // 新一次 attach 必须**不带** `stream_resume`：断线重连仅由 `send_chat_stream` 内部循环
            // 用响应头里的 `x-stream-job-id` 与 `last_event_id` 完成。若此处读取 UI 上残留的
            // `stream_job_id`（例如上轮 SSE 报错未收到 `stream_ended`），会误用已 `remove_job` 的
            // id，首包即 410「无法重连」。
            stream_job_id_sig.set(None);
            stream_last_event_seq_sig.set(0);
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
                sessions,
                locale: locale_sig,
                active_session_id: active_id.get(),
                assistant_message_id: asst_id.clone(),
                abort_cell: Arc::clone(&abort_cell),
                user_cancelled_stream: Arc::clone(&user_cancelled_stream),
                status_busy,
                status_err,
                tool_busy,
                pending_approval,
                approval_session_store_id: appr_store.clone(),
                session_sync,
                session_hydrate_nonce,
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
                    stream_ctx.sessions.update(|list| {
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
                    stream_ctx.sessions.update(|list| {
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
                })
            };
            let on_error: Rc<dyn Fn(String)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                let stream_job_id_sig = stream_job_id_sig;
                let stream_last_event_seq_sig = stream_last_event_seq_sig;
                Rc::new(move |msg: String| {
                    if *stream_ctx.user_cancelled_stream.lock().unwrap() {
                        *stream_ctx.abort_cell.lock().unwrap() = None;
                        return;
                    }
                    stream_job_id_sig.set(None);
                    stream_last_event_seq_sig.set(0);
                    let aid = stream_ctx.active_session_id.clone();
                    let mid = stream_ctx.assistant_message_id.clone();
                    stream_ctx.sessions.update(|list| {
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
                    stream_ctx.sessions.update(|list| {
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
                        .session_sync
                        .update(|s| s.apply_stream_conversation_id(id.clone()));
                    let aid = stream_ctx.active_session_id.clone();
                    stream_ctx.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                            s.server_conversation_id = Some(id);
                            s.server_revision = None;
                        }
                    });
                    stream_ctx
                        .session_hydrate_nonce
                        .update(|n| *n = n.wrapping_add(1));
                })
            };
            let on_conv_rev: Rc<dyn Fn(u64)> = {
                let stream_ctx = Rc::clone(&stream_ctx);
                Rc::new(move |rev: u64| {
                    stream_ctx
                        .session_sync
                        .update(|s| s.apply_saved_revision(rev));
                    let aid = stream_ctx.active_session_id.clone();
                    stream_ctx.sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|x| x.id == aid) {
                            s.server_revision = Some(rev);
                        }
                    });
                    stream_ctx
                        .session_hydrate_nonce
                        .update(|n| *n = n.wrapping_add(1));
                })
            };

            let on_stream_ended: Rc<dyn Fn(String)> = {
                let stream_job_id_sig = stream_job_id_sig;
                let stream_last_event_seq_sig = stream_last_event_seq_sig;
                Rc::new(move |reason: String| {
                    if reason == "completed" || reason == "cancelled" {
                        stream_job_id_sig.set(None);
                        stream_last_event_seq_sig.set(0);
                    }
                })
            };
            let on_stream_job_id: Rc<dyn Fn(u64)> = {
                let stream_job_id_sig = stream_job_id_sig;
                Rc::new(move |jid: u64| {
                    stream_job_id_sig.set(Some(jid));
                })
            };
            let on_last_sse_event_id: Rc<dyn Fn(u64)> = {
                let stream_last_event_seq_sig = stream_last_event_seq_sig;
                Rc::new(move |seq: u64| {
                    stream_last_event_seq_sig.set(seq);
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
                    stream_ctx.sessions.update(|list| {
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
                    stream_ctx.sessions.update(|list| {
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
    });

    let run_send_message: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let pending_clarification_sig = pending_clarification;
        let locale_sig = locale;
        move || {
            let text = composer_draft_buffer.lock().unwrap().trim().to_string();
            let imgs = pending_images.get();
            let loc = locale_sig.get();
            let (user_line, clarify_json) = if let Some(form) = pending_clarification_sig.get() {
                let mut answers = serde_json::Map::new();
                let mut ok = true;
                for (i, f) in form.fields.iter().enumerate() {
                    let v = form
                        .values
                        .get(i)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    if f.required && v.is_empty() {
                        ok = false;
                        break;
                    }
                    answers.insert(f.id.clone(), serde_json::Value::String(v));
                }
                if !ok {
                    status_err.set(Some(i18n::clarification_missing_required(loc).to_string()));
                    return;
                }
                let qid = form.questionnaire_id.clone();
                pending_clarification_sig.set(None);
                let cq = serde_json::json!({
                    "questionnaire_id": qid,
                    "answers": serde_json::Value::Object(answers),
                });
                (
                    i18n::clarification_user_bubble_stub(loc).to_string(),
                    Some(cq),
                )
            } else {
                (text, None)
            };
            if (user_line.is_empty() && imgs.is_empty() && clarify_json.is_none())
                || !initialized.get()
                || status_busy.get()
            {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            let imgs_send = imgs.clone();
            patch_active_session(sessions, &active_id.get(), |s| {
                let now = message_created_ms();
                let is_first_user_turn =
                    s.messages.iter().filter(|m| m.role == "user").count() == 0;
                s.messages.push(StoredMessage {
                    id: uid.clone(),
                    role: "user".to_string(),
                    text: user_line.clone(),
                    reasoning_text: String::new(),
                    image_urls: imgs_send.clone(),
                    state: None,
                    is_tool: false,
                    created_at: now,
                });
                s.messages.push(StoredMessage {
                    id: asst_id.clone(),
                    role: "assistant".to_string(),
                    text: String::new(),
                    reasoning_text: String::new(),
                    image_urls: vec![],
                    state: Some("loading".to_string()),
                    is_tool: false,
                    created_at: now,
                });
                if is_first_user_turn && i18n::is_default_session_title(&s.title) {
                    s.title = title_from_user_prompt(&user_line);
                }
                s.draft.clear();
            });
            draft.set(String::new());
            *composer_draft_buffer.lock().unwrap() = String::new();
            pending_images.set(Vec::new());
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_line, imgs_send, asst_id, clarify_json);
        }
    });

    let retry_assistant_target = RwSignal::new(None::<String>);
    let regen_stream_after_truncate = RwSignal::new(None::<(String, Vec<String>, String)>);

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move |_| {
            let Some(failed_asst_id) = retry_assistant_target.get() else {
                return;
            };
            // 先消费信号，避免在 `status_busy` 等依赖触发下反复入队同一次重试。
            retry_assistant_target.set(None);
            if !initialized.get() || status_busy.get() {
                return;
            }
            let aid = active_id.get();
            let mut prepared: Option<(String, Vec<String>, String)> = None;
            sessions.update(|list| {
                prepared = prepare_retry_failed_assistant_turn(list, &aid, &failed_asst_id);
            });
            let Some((user_text, user_imgs, asst_id)) = prepared else {
                return;
            };
            auto_scroll_chat.set(true);
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_text, user_imgs, asst_id, None);
        }
    });

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move |_| {
            let Some((user_text, user_imgs, asst_id)) = regen_stream_after_truncate.get() else {
                return;
            };
            regen_stream_after_truncate.set(None);
            if !initialized.get() || status_busy.get() {
                return;
            }
            auto_scroll_chat.set(true);
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_text, user_imgs, asst_id, None);
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> =
        Arc::new({
            let abort_cell = Arc::clone(&abort_cell);
            let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
            let locale = locale;
            move || {
                if abort_cell.lock().unwrap().is_none() {
                    return;
                }
                *user_cancelled_stream.lock().unwrap() = true;
                if let Some(ac) = abort_cell.lock().unwrap().take() {
                    ac.abort();
                }
                let loc = locale.get_untracked();
                let aid = active_id.get();
                sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        if let Some(m) = s.messages.iter_mut().rev().find(|m| {
                            m.role == "assistant" && m.state.as_deref() == Some("loading")
                        }) {
                            m.state = None;
                            if m.text.trim().is_empty() {
                                m.text = i18n::stream_stopped_inline(loc).to_string();
                            } else {
                                m.text.push_str(i18n::stream_stopped_suffix(loc));
                            }
                        }
                    }
                });
                status_busy.set(false);
                tool_busy.set(false);
            }
        });

    let new_session: Rc<dyn Fn()> = Rc::new({
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let session_sync = session_sync;
        move || {
            let prev = active_id.get_untracked();
            if !prev.is_empty() {
                let buf = composer_draft_buffer.lock().unwrap().clone();
                flush_composer_draft_to_session(sessions, &prev, &buf);
            }
            let now = js_sys::Date::now() as i64;
            let s = ChatSession {
                id: make_session_id(),
                title: DEFAULT_CHAT_SESSION_TITLE.to_string(),
                draft: String::new(),
                messages: vec![],
                updated_at: now,
                pinned: false,
                starred: false,
                server_conversation_id: None,
                server_revision: None,
            };
            let id = s.id.clone();
            sessions.update(|list| {
                list.insert(0, s);
            });
            active_id.set(id);
            draft.set(String::new());
            session_sync.set(SessionSyncState::local_only());
        }
    });

    ChatComposerWires {
        retry_assistant_target,
        regen_stream_after_truncate,
        run_send_message,
        cancel_stream,
        new_session,
    }
}
