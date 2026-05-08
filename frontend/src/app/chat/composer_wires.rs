//! [`super::handles::WireComposerStreamsArgs`] → [`super::handles::ChatComposerWires`] 的接线实现，降低 `composer` 单文件圈复杂度。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use super::composer_stream::{
    ComposerStreamHandles, make_attach_chat_stream, user_cancel_in_flight_stream,
};
use super::handles::{ChatComposerWires, ComposerStreamShell, WireComposerStreamsArgs};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::i18n::Locale;
use crate::session_ops::{
    flush_active_composer_draft, make_message_id, message_created_ms, patch_active_session,
    prepare_retry_failed_assistant_turn, title_from_user_prompt,
};
use crate::session_sync::SessionSyncState;
use crate::storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, StoredMessageState, make_session_id,
};

fn user_line_and_clarify_from_shell(
    shell: &ComposerStreamShell,
    trimmed_draft: &str,
    loc: Locale,
) -> Option<(String, Option<serde_json::Value>)> {
    if let Some(form) = shell.approval.pending_clarification.get() {
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
                .stream
                .status_err
                .set(Some(i18n::clarification_missing_required(loc).to_string()));
            return None;
        }
        let qid = form.questionnaire_id.clone();
        shell.approval.pending_clarification.set(None);
        let cq = serde_json::json!({
            "questionnaire_id": qid,
            "answers": serde_json::Value::Object(answers),
        });
        Some((
            i18n::clarification_user_bubble_stub(loc).to_string(),
            Some(cq),
        ))
    } else {
        Some((trimmed_draft.to_string(), None))
    }
}

fn push_user_and_loading_assistant(
    chat: ChatSessionSignals,
    user_line: String,
    imgs_send: Vec<String>,
    uid: String,
    asst_id: String,
) {
    patch_active_session(chat.sessions, &chat.active_id.get(), |s| {
        let now = message_created_ms();
        let is_first_user_turn = s.messages.iter().filter(|m| m.role == "user").count() == 0;
        s.messages.push(StoredMessage {
            id: uid.clone(),
            role: "user".to_string(),
            text: user_line.clone(),
            reasoning_text: String::new(),
            image_urls: imgs_send.clone(),
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
        s.messages.push(StoredMessage {
            id: asst_id.clone(),
            role: "assistant".to_string(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: now,
        });
        if is_first_user_turn && i18n::is_default_session_title(&s.title) {
            s.title = title_from_user_prompt(&user_line);
        }
        s.draft.clear();
    });
}

fn begin_stream_shell_turn(shell: &ComposerStreamShell) {
    shell.stream.status_busy.set(true);
    shell.stream.status_err.set(None);
    shell.approval.pending_approval.set(None);
}

/// 用户中止流式：收尾 **assistant** 与 **工具** 的 `Loading` 占位，避免时间线仍判为「工具执行中」。
fn finalize_stream_loading_placeholders_after_user_abort(
    chat: ChatSessionSignals,
    aid: String,
    loc: Locale,
) {
    let running_label = i18n::status_tool_running(loc);
    let stopped_tool = i18n::status_tool_stopped_user(loc);
    chat.update_sessions_composer(|list| {
        let Some(s) = list.iter_mut().find(|s| s.id == aid) else {
            return;
        };
        if let Some(m) = s.messages.iter_mut().rev().find(|m| {
            m.role == "assistant"
                && !m.is_tool
                && m.state.as_ref().is_some_and(|st| st.is_loading())
        }) {
            m.state = None;
            if m.text.trim().is_empty() {
                m.text = i18n::stream_stopped_inline(loc).to_string();
            } else {
                m.text.push_str(i18n::stream_stopped_suffix(loc));
            }
        }
        for m in &mut s.messages {
            if !m.is_tool || !m.state.as_ref().is_some_and(|st| st.is_loading()) {
                continue;
            }
            m.state = None;
            if m.reasoning_text.contains("status: running") {
                m.reasoning_text = m
                    .reasoning_text
                    .replace("status: running", "status: stopped (user)");
            }
            if m.text.contains(running_label) {
                m.text = m.text.replacen(running_label, stopped_tool, 1);
            } else if m.text.trim().is_empty() {
                m.text = i18n::stream_stopped_inline(loc).to_string();
            } else {
                m.text.push_str(i18n::stream_stopped_suffix(loc));
            }
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
        stream_turn_busy_ui,
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
        let shell = stream_shell.clone();
        let locale_sig = locale;
        move || {
            let text = draft.get_untracked().trim().to_string();
            let imgs = pending_images.get();
            let loc = locale_sig.get();
            let Some((user_line, clarify_json)) =
                user_line_and_clarify_from_shell(&shell, &text, loc)
            else {
                return;
            };
            if (user_line.is_empty() && imgs.is_empty() && clarify_json.is_none())
                || !initialized.get()
                || stream_turn_busy_ui.get()
            {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            let imgs_send = imgs.clone();
            push_user_and_loading_assistant(
                chat,
                user_line.clone(),
                imgs_send.clone(),
                uid,
                asst_id.clone(),
            );
            draft.set(String::new());
            pending_images.set(Vec::new());
            begin_stream_shell_turn(&shell);
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
            retry_assistant_target.set(None);
            if !initialized.get() || stream_turn_busy_ui.get() {
                return;
            }
            let aid = chat.active_id.get();
            let mut prepared: Option<(String, Vec<String>, String)> = None;
            chat.update_sessions_composer(|list| {
                prepared = prepare_retry_failed_assistant_turn(list, &aid, &failed_asst_id);
            });
            let Some((user_text, user_imgs, asst_id)) = prepared else {
                return;
            };
            auto_scroll_chat.set(true);
            begin_stream_shell_turn(&shell);
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
            let busy = stream_turn_busy_ui.get();
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
            begin_stream_shell_turn(&shell);
            attach(user_text, user_imgs, asst_id, None);
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let chat = chat;
        let shell = stream_shell.clone();
        let locale = locale;
        move || {
            if !user_cancel_in_flight_stream(&shell) {
                return;
            }
            let loc = locale.get_untracked();
            let aid = chat.effective_stream_message_session_id();
            finalize_stream_loading_placeholders_after_user_abort(chat, aid, loc);
            shell.stream.status_busy.set(false);
            shell.stream.tool_busy.set(false);
        }
    });

    let new_session: Rc<dyn Fn()> = Rc::new({
        let chat = chat;
        move || {
            flush_active_composer_draft(chat.sessions, chat.active_id, draft);
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
            chat.update_sessions_composer(|list| {
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
