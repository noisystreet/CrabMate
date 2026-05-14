//! [`super::handles::WireComposerStreamsArgs`] → [`super::handles::ChatComposerWires`] 的接线实现，降低 `composer` 单文件圈复杂度。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use super::composer_follow_up::ComposerStreamFollowUp;
use super::composer_stream::{ComposerStreamHandles, make_attach_chat_stream};
use super::handles::{ChatComposerWires, ComposerStreamShell, WireComposerStreamsArgs};
use super::stream_user_abort::apply_user_abort_of_inflight_stream;
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

/// 截断后再生：是否因「真在跑的流 / 工具 / abort / 其它助手 Loading」应暂缓 `attach`（**不计** `asst_id` 自身占位）。
fn regen_stream_blocked_for_attach(
    shell: &ComposerStreamShell,
    chat: ChatSessionSignals,
    asst_id: &str,
    user_text_len: usize,
) -> bool {
    let status_busy = shell.stream.status_busy.get();
    let tool_busy = shell.stream.tool_busy.get();
    let abort_present = shell
        .stream
        .abort_cell
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false);
    let conflict_loading =
        crate::chat_session_state::session_has_conflicting_stream_loading_placeholders(
            chat, asst_id,
        );
    web_sys::console::log_1(
        &format!(
            "[effect] regen_stream: status_busy={status_busy}, tool_busy={tool_busy}, abort={abort_present}, conflict_loading={conflict_loading}, text_len={user_text_len}, asst_id={asst_id}",
        )
        .into(),
    );
    status_busy || tool_busy || abort_present || conflict_loading
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

    let stream_follow_up = RwSignal::new(ComposerStreamFollowUp::Idle);

    Effect::new({
        let chat = chat;
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let shell = stream_shell.clone();
        move |_| {
            let pending = stream_follow_up.get();
            match pending {
                ComposerStreamFollowUp::Idle => {}
                ComposerStreamFollowUp::RetryFailedAssistant { failed_asst_id } => {
                    if !initialized.get() || stream_turn_busy_ui.get() {
                        return;
                    }
                    stream_follow_up.set(ComposerStreamFollowUp::Idle);
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
                ComposerStreamFollowUp::RegenerateAfterTruncate {
                    user_text,
                    user_imgs,
                    asst_id,
                } => {
                    if !initialized.get() {
                        return;
                    }
                    if regen_stream_blocked_for_attach(
                        &shell,
                        chat,
                        asst_id.as_str(),
                        user_text.len(),
                    ) {
                        return;
                    }
                    stream_follow_up.set(ComposerStreamFollowUp::Idle);
                    auto_scroll_chat.set(true);
                    begin_stream_shell_turn(&shell);
                    attach(user_text, user_imgs, asst_id, None);
                }
            }
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let chat = chat;
        let shell = stream_shell.clone();
        let locale = locale;
        move || {
            let loc = locale.get_untracked();
            let _ = apply_user_abort_of_inflight_stream(chat, &shell, loc);
        }
    });

    let new_session: Rc<dyn Fn()> = Rc::new({
        let chat = chat;
        move || {
            flush_active_composer_draft(chat.sessions, chat.active_id, draft);
            let prev_id = chat.active_id.get_untracked();
            let inherited_ws = chat.sessions.with_untracked(|list| {
                list.iter()
                    .find(|s| s.id == prev_id)
                    .and_then(|s| s.workspace_root.clone())
            });
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
                workspace_root: inherited_ws,
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
        stream_follow_up,
        run_send_message,
        cancel_stream,
        new_session,
    }
}
