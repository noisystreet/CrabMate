//! 输入区与流式对话：草稿缓冲、发送 / 停止、重试 / 截断再生、新会话。

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use crate::api::{ChatStreamCallbacks, send_chat_stream};
use crate::message_format::tool_card_text;
use crate::session_ops::{
    approval_session_id, flush_composer_draft_to_session, make_message_id, message_created_ms,
    patch_active_session, prepare_retry_failed_assistant_turn, title_from_user_prompt,
};
use crate::sse_dispatch::{CommandApprovalRequest, ToolResultInfo};
use crate::storage::{ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, make_session_id};

pub(super) struct ChatComposerWires {
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, String)>>,
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
    conversation_id: RwSignal<Option<String>>,
    conversation_revision: RwSignal<Option<u64>>,
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
        conversation_id.set(None);
        conversation_revision.set(None);
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
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    conversation_revision: RwSignal<Option<u64>>,
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
) -> ChatComposerWires {
    let attach_chat_stream: Arc<dyn Fn(String, String) + Send + Sync> = Arc::new({
        let abort_cell = Arc::clone(&abort_cell);
        let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
        let sessions = sessions;
        let active_id = active_id;
        let conversation_id = conversation_id;
        let conversation_revision = conversation_revision;
        let selected_agent_role = selected_agent_role;
        let status_busy = status_busy;
        let status_err = status_err;
        let pending_approval = pending_approval;
        let tool_busy = tool_busy;
        let refresh_workspace = Arc::clone(&refresh_workspace);
        let changelist_modal_open = changelist_modal_open;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        move |user_text: String, asst_id: String| {
            if let Some(prev) = abort_cell.lock().unwrap().take() {
                prev.abort();
            }
            *user_cancelled_stream.lock().unwrap() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *abort_cell.lock().unwrap() = Some(ac);

            let conv = conversation_id.get();
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Arc::clone(&user_cancelled_stream);

            let on_delta: Rc<dyn Fn(String)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                Rc::new(move |chunk: String| {
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id) {
                                m.text.push_str(&chunk);
                            }
                        }
                    });
                })
            };
            let on_done: Rc<dyn Fn()> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                let abort_cell = Arc::clone(&abort_cell);
                let user_cancelled_stream = Arc::clone(&user_cancelled_for_spawn);
                Rc::new(move || {
                    if *user_cancelled_stream.lock().unwrap() {
                        *abort_cell.lock().unwrap() = None;
                        return;
                    }
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act)
                            && let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id)
                            && m.state.as_deref() == Some("loading")
                        {
                            // 仅收尾「仍在生成」的气泡；SSE 已 on_error 的勿覆盖 error 状态
                            m.state = None;
                            if m.text.trim().is_empty() {
                                m.text = "(无回复)".to_string();
                            }
                        }
                    });
                    status_busy.set(false);
                    *abort_cell.lock().unwrap() = None;
                })
            };
            let on_error: Rc<dyn Fn(String)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                let abort_cell = Arc::clone(&abort_cell);
                let user_cancelled_stream = Arc::clone(&user_cancelled_for_spawn);
                Rc::new(move |msg: String| {
                    if *user_cancelled_stream.lock().unwrap() {
                        *abort_cell.lock().unwrap() = None;
                        return;
                    }
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id) {
                                m.text = msg;
                                m.state = Some("error".to_string());
                            }
                        }
                    });
                    status_busy.set(false);
                    status_err.set(Some("对话失败".to_string()));
                    *abort_cell.lock().unwrap() = None;
                })
            };
            let on_ws: Rc<dyn Fn()> = {
                let changelist_modal_open = changelist_modal_open;
                let changelist_fetch_nonce = changelist_fetch_nonce;
                let refresh_workspace = Arc::clone(&refresh_workspace);
                Rc::new(move || {
                    refresh_workspace();
                    if changelist_modal_open.get_untracked() {
                        changelist_fetch_nonce.update(|x| *x = x.wrapping_add(1));
                    }
                })
            };
            let on_tool_status: Rc<dyn Fn(bool)> = {
                let tool_busy = tool_busy;
                Rc::new(move |b: bool| {
                    tool_busy.set(b);
                })
            };
            let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                Rc::new(move |info: ToolResultInfo| {
                    let t = tool_card_text(&info);
                    let id = make_message_id();
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            s.messages.push(StoredMessage {
                                id,
                                role: "system".to_string(),
                                text: t,
                                state: None,
                                is_tool: true,
                                created_at: message_created_ms(),
                            });
                        }
                    });
                })
            };
            let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
                let pending_approval = pending_approval;
                let sid = appr_store.clone();
                Rc::new(move |req: CommandApprovalRequest| {
                    pending_approval.set(Some((sid.clone(), req.command, req.args)));
                })
            };
            let on_cid: Rc<dyn Fn(String)> = {
                let conversation_id = conversation_id;
                let conversation_revision = conversation_revision;
                Rc::new(move |id: String| {
                    conversation_id.set(Some(id));
                    conversation_revision.set(None);
                })
            };
            let on_conv_rev: Rc<dyn Fn(u64)> = {
                let conversation_revision = conversation_revision;
                Rc::new(move |rev: u64| {
                    conversation_revision.set(Some(rev));
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
            };

            spawn_local(async move {
                let stream_result = send_chat_stream(
                    user_text,
                    conv,
                    agent_role,
                    Some(appr_for_stream),
                    &signal,
                    cbs.clone(),
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
        move || {
            let text = composer_draft_buffer.lock().unwrap().trim().to_string();
            if text.is_empty() || !initialized.get() || status_busy.get() {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            patch_active_session(sessions, &active_id.get(), |s| {
                let now = message_created_ms();
                let is_first_user_turn =
                    s.messages.iter().filter(|m| m.role == "user").count() == 0;
                s.messages.push(StoredMessage {
                    id: uid.clone(),
                    role: "user".to_string(),
                    text: text.clone(),
                    state: None,
                    is_tool: false,
                    created_at: now,
                });
                s.messages.push(StoredMessage {
                    id: asst_id.clone(),
                    role: "assistant".to_string(),
                    text: String::new(),
                    state: Some("loading".to_string()),
                    is_tool: false,
                    created_at: now,
                });
                if is_first_user_turn && s.title == DEFAULT_CHAT_SESSION_TITLE {
                    s.title = title_from_user_prompt(&text);
                }
                s.draft.clear();
            });
            draft.set(String::new());
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(text, asst_id);
        }
    });

    let retry_assistant_target = RwSignal::new(None::<String>);
    let regen_stream_after_truncate = RwSignal::new(None::<(String, String)>);

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
            let mut prepared: Option<(String, String)> = None;
            sessions.update(|list| {
                prepared = prepare_retry_failed_assistant_turn(list, &aid, &failed_asst_id);
            });
            let Some((user_text, asst_id)) = prepared else {
                return;
            };
            auto_scroll_chat.set(true);
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_text, asst_id);
        }
    });

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move |_| {
            let Some((user_text, asst_id)) = regen_stream_after_truncate.get() else {
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
            attach(user_text, asst_id);
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> =
        Arc::new({
            let abort_cell = Arc::clone(&abort_cell);
            let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
            move || {
                if abort_cell.lock().unwrap().is_none() {
                    return;
                }
                *user_cancelled_stream.lock().unwrap() = true;
                if let Some(ac) = abort_cell.lock().unwrap().take() {
                    ac.abort();
                }
                let aid = active_id.get();
                sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        if let Some(m) = s.messages.iter_mut().rev().find(|m| {
                            m.role == "assistant" && m.state.as_deref() == Some("loading")
                        }) {
                            m.state = None;
                            if m.text.trim().is_empty() {
                                m.text = "已停止".to_string();
                            } else {
                                m.text.push_str("\n\n[已停止]");
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
            };
            let id = s.id.clone();
            sessions.update(|list| {
                list.insert(0, s);
            });
            active_id.set(id);
            draft.set(String::new());
            conversation_id.set(None);
            conversation_revision.set(None);
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
