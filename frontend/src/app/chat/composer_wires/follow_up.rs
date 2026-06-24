//! 截断再生与失败助手重试：单一 `Effect` 消费 [`ComposerStreamFollowUp`](super::super::composer_follow_up::ComposerStreamFollowUp)。

use std::sync::Arc;

use leptos::prelude::*;

use super::super::composer_follow_up::ComposerStreamFollowUp;
use super::super::composer_stream::AttachChatStreamArc;
use super::super::handles::ComposerStreamShell;
use super::super::scroll_follow::engage_follow_and_scroll_bottom;
use super::super::stream_follow_up_gates::RegenAttachGate;
use super::helpers::begin_stream_shell_turn;
use crate::app::chat::scroll_shell::ChatScrollShellSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::session_ops::prepare_retry_failed_assistant_turn;

/// [`wire_stream_follow_up_effect`] 入参聚合（压低形参个数与 clippy `type_complexity`）。
pub(super) struct StreamFollowUpWiring {
    pub initialized: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub attach_chat_stream: AttachChatStreamArc,
    pub scroll_shell: ChatScrollShellSignals,
    pub shell: ComposerStreamShell,
    pub stream_follow_up: RwSignal<ComposerStreamFollowUp>,
    pub stream_turn_busy_ui: Memo<bool>,
}

pub(super) fn wire_stream_follow_up_effect(args: StreamFollowUpWiring) {
    let StreamFollowUpWiring {
        initialized,
        chat,
        attach_chat_stream,
        scroll_shell,
        shell,
        stream_follow_up,
        stream_turn_busy_ui,
    } = args;

    Effect::new({
        let chat = chat;
        let attach = Arc::clone(&attach_chat_stream);
        let scroll_shell = scroll_shell;
        let shell = shell.clone();
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
                    engage_follow_and_scroll_bottom(scroll_shell);
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
                    if RegenAttachGate::capture(&shell, chat, asst_id.as_str()).is_blocked() {
                        return;
                    }
                    stream_follow_up.set(ComposerStreamFollowUp::Idle);
                    engage_follow_and_scroll_bottom(scroll_shell);
                    begin_stream_shell_turn(&shell);
                    attach(user_text, user_imgs, asst_id, None);
                }
            }
        }
    });
}
