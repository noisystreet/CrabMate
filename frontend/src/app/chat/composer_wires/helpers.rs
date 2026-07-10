//! 发送路径纯函数：澄清问卷折叠、用户行 + Loading 尾泡、流式壳忙态入口。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n;
use crate::i18n::Locale;
use crate::session_ops::{message_created_ms, patch_active_session, title_from_user_prompt};
use crate::storage::{StoredMessage, StoredMessageState};

use super::super::handles::ComposerStreamShell;

pub(super) fn user_line_and_clarify_from_shell(
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

pub(super) fn push_user_and_loading_assistant(
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

pub(super) fn begin_stream_shell_turn(shell: &ComposerStreamShell) {
    shell.stream.status_err.set(None);
    shell.approval.clear_pending_user_interactions();
}
