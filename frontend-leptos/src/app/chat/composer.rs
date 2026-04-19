//! 输入区与流式对话：草稿缓冲、发送 / 停止、重试 / 截断再生、新会话。
//!
//! `/chat/stream` 的 SSE 回调装配见 [`super::composer_stream`]。

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use gloo_timers::future::TimeoutFuture;

use super::composer_stream::{ComposerStreamHandles, make_attach_chat_stream};
use super::handles::WireComposerStreamsArgs;
use crate::clarification_form::PendingClarificationForm;
use crate::i18n;
use crate::session_ops::{
    flush_composer_draft_to_session, make_message_id, message_created_ms, patch_active_session,
    prepare_retry_failed_assistant_turn, title_from_user_prompt,
};
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, make_session_id};

use crate::chat_session_state::ChatSessionSignals;

pub(crate) struct ChatComposerWires {
    pub retry_assistant_target: RwSignal<Option<String>>,
    pub regen_stream_after_truncate: RwSignal<Option<(String, Vec<String>, String)>>,
    pub run_send_message: Arc<dyn Fn() + Send + Sync>,
    pub cancel_stream: Arc<dyn Fn() + Send + Sync>,
    pub new_session: Rc<dyn Fn()>,
}

/// 切换会话时重置会话级 UI 状态并加载该会话草稿（勿订阅 `sessions`，避免流式更新覆盖缓冲）。
pub(crate) fn wire_session_switch_clears_chat_state(
    initialized: RwSignal<bool>,
    chat: ChatSessionSignals,
    draft: RwSignal<String>,
    pending_images: RwSignal<Vec<String>>,
    pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    collapsed_long_assistant_ids: RwSignal<Vec<String>>,
) {
    Effect::new(move |_| {
        let id = chat.active_id.get();
        if !initialized.get() {
            return;
        }
        let list = chat.sessions.get_untracked();
        let d = list
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        draft.set(d);
        pending_images.set(Vec::new());
        pending_clarification.set(None);
        // 仅用上方 `list`（get_untracked）：勿再 `sessions.with`，否则 effect 会订阅流式
        // `sessions` 更新。
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
        chat.session_sync
            .set(st.unwrap_or_else(SessionSyncState::local_only));
        chat.stream_job_id.set(None);
        chat.stream_last_event_seq.set(0);
        collapsed_long_assistant_ids.set(Vec::new());
    });
}

/// `draft` 程序化更新时同步 Mutex 与 textarea（输入过程不订阅 `draft`）。
pub(crate) fn wire_draft_sync_to_buffer_and_textarea(
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
                if let Some(el) = cref.get_untracked() {
                    if el.value() != d_for_dom {
                        el.set_value(&d_for_dom);
                    }
                }
            });
        }
    });
}

pub(crate) fn wire_chat_composer_streams(args: WireComposerStreamsArgs) -> ChatComposerWires {
    let WireComposerStreamsArgs {
        initialized,
        chat,
        locale,
        draft,
        selected_agent_role,
        stream_shell,
        composer_draft_buffer,
        auto_scroll_chat,
        pending_images,
    } = args;

    let stream_shell_for_attach = stream_shell.clone();
    let attach_chat_stream = make_attach_chat_stream(ComposerStreamHandles {
        chat,
        locale,
        selected_agent_role,
        shell: stream_shell_for_attach,
    });

    let run_send_message: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let chat = chat;
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let shell = stream_shell.clone();
        let locale_sig = locale;
        move || {
            let text = composer_draft_buffer.lock().unwrap().trim().to_string();
            let imgs = pending_images.get();
            let loc = locale_sig.get();
            let (user_line, clarify_json) = if let Some(form) = shell.pending_clarification.get() {
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
                    shell
                        .status_err
                        .set(Some(i18n::clarification_missing_required(loc).to_string()));
                    return;
                }
                let qid = form.questionnaire_id.clone();
                shell.pending_clarification.set(None);
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
                || shell.status_busy.get()
            {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            let imgs_send = imgs.clone();
            patch_active_session(chat.sessions, &chat.active_id.get(), |s| {
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
            shell.status_busy.set(true);
            shell.status_err.set(None);
            shell.pending_approval.set(None);
            attach(user_line, imgs_send, asst_id, clarify_json);
        }
    });

    let retry_assistant_target = RwSignal::new(None::<String>);
    let regen_stream_after_truncate = RwSignal::new(None::<(String, Vec<String>, String)>);

    Effect::new({
        let chat = chat;
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let shell = stream_shell.clone();
        move |_| {
            let Some(failed_asst_id) = retry_assistant_target.get() else {
                return;
            };
            // 先消费信号，避免在 `status_busy` 等依赖触发下反复入队同一次重试。
            retry_assistant_target.set(None);
            if !initialized.get() || shell.status_busy.get() {
                return;
            }
            let aid = chat.active_id.get();
            let mut prepared: Option<(String, Vec<String>, String)> = None;
            chat.sessions.update(|list| {
                prepared = prepare_retry_failed_assistant_turn(list, &aid, &failed_asst_id);
            });
            let Some((user_text, user_imgs, asst_id)) = prepared else {
                return;
            };
            auto_scroll_chat.set(true);
            shell.status_busy.set(true);
            shell.status_err.set(None);
            shell.pending_approval.set(None);
            attach(user_text, user_imgs, asst_id, None);
        }
    });

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let shell = stream_shell.clone();
        move |_| {
            let Some((user_text, user_imgs, asst_id)) = regen_stream_after_truncate.get() else {
                return;
            };
            regen_stream_after_truncate.set(None);
            let init = initialized.get();
            let busy = shell.status_busy.get();
            web_sys::console::log_1(
                &format!(
                    "[effect] regen_stream consumed: init={}, busy={}, text={}, asst_id={}",
                    init, busy, user_text, asst_id
                )
                .into(),
            );
            if !init || busy {
                return;
            }
            auto_scroll_chat.set(true);
            shell.status_busy.set(true);
            shell.status_err.set(None);
            shell.pending_approval.set(None);
            attach(user_text, user_imgs, asst_id, None);
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> =
        Arc::new({
            let chat = chat;
            let shell = stream_shell.clone();
            let locale = locale;
            move || {
                if shell.abort_cell.lock().unwrap().is_none() {
                    return;
                }
                *shell.user_cancelled_stream.lock().unwrap() = true;
                if let Some(ac) = shell.abort_cell.lock().unwrap().take() {
                    ac.abort();
                }
                let loc = locale.get_untracked();
                let aid = chat.active_id.get();
                chat.sessions.update(|list| {
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
                shell.status_busy.set(false);
                shell.tool_busy.set(false);
            }
        });

    let new_session: Rc<dyn Fn()> = Rc::new({
        let chat = chat;
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        move || {
            let prev = chat.active_id.get_untracked();
            if !prev.is_empty() {
                let buf = composer_draft_buffer.lock().unwrap().clone();
                flush_composer_draft_to_session(chat.sessions, &prev, &buf);
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
            chat.sessions.update(|list| {
                list.insert(0, s);
            });
            chat.active_id.set(id);
            draft.set(String::new());
            chat.session_sync.set(SessionSyncState::local_only());
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
