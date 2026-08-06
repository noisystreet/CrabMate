//! 终端流回合操作：可用动作列表与点击分发（UI 为右键 / 长按菜单，见 `message_turn_menu`）。

use leptos::prelude::*;

use super::composer_follow_up::ComposerStreamFollowUp;
use super::message_row_actions::MessageRowActionSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::write_clipboard_text;
use crate::storage::StoredMessage;
use crate::stream_text_overlay::message_text_for_display_including_stream_overlay;

/// 是否可对该回合打开操作菜单（工具卡无）。
#[must_use]
pub(crate) fn turn_actions_visible(message: &StoredMessage) -> bool {
    !message.is_tool
}

fn is_user_plain(message: &StoredMessage) -> bool {
    message.role == "user" && !message.is_tool
}

fn is_failed_assistant(message: &StoredMessage) -> bool {
    message.role == "assistant"
        && !message.is_tool
        && message.state.as_ref().is_some_and(|s| s.is_error())
}

/// 该回合菜单可展示的动作键（顺序即菜单顺序）。
#[must_use]
pub(crate) fn turn_menu_action_keys(message: &StoredMessage) -> Vec<&'static str> {
    if !turn_actions_visible(message) {
        return Vec::new();
    }
    let mut keys = vec!["copy"];
    if is_user_plain(message) {
        keys.push("regen");
        keys.push("branch");
    }
    if is_failed_assistant(message) {
        keys.push("retry");
    }
    keys
}

/// 点击分发所需信号。
#[derive(Clone, Copy)]
pub(crate) struct TuiTurnActionHandlers {
    pub chat: ChatSessionSignals,
    pub locale: RwSignal<Locale>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub stream_follow_up: RwSignal<ComposerStreamFollowUp>,
    pub stream_turn_busy_ui: Memo<bool>,
    pub status_err: RwSignal<Option<String>>,
}

fn copy_message_by_id(handlers: TuiTurnActionHandlers, message_id: &str) {
    let loc = handlers.locale.get_untracked();
    let apply = handlers.apply_assistant_display_filters.get_untracked();
    let ov = handlers.chat.stream_text_overlay.get_untracked();
    let text = handlers.chat.sessions.with(|list| {
        let aid = handlers.chat.active_id.get_untracked();
        list.iter()
            .find(|s| s.id == aid)
            .and_then(|s| s.messages.iter().find(|m| m.id == message_id))
            .map(|msg| {
                message_text_for_display_including_stream_overlay(
                    msg,
                    ov.as_ref(),
                    aid.as_str(),
                    loc,
                    apply,
                )
            })
            .unwrap_or_default()
    });
    write_clipboard_text(&text, loc);
}

/// 处理回合动作；返回是否已消费。
pub(crate) fn dispatch_tui_turn_action(
    handlers: TuiTurnActionHandlers,
    action: &str,
    message_id: &str,
    msg_idx: usize,
) -> bool {
    match action {
        "copy" => {
            copy_message_by_id(handlers, message_id);
            true
        }
        "retry" => {
            if handlers.stream_turn_busy_ui.get_untracked() {
                return true;
            }
            handlers
                .stream_follow_up
                .set(ComposerStreamFollowUp::RetryFailedAssistant {
                    failed_asst_id: message_id.to_string(),
                });
            true
        }
        "regen" => {
            if handlers.stream_turn_busy_ui.get_untracked() {
                return true;
            }
            let row_actions = MessageRowActionSignals {
                chat: handlers.chat,
                stream_follow_up: handlers.stream_follow_up,
                status_err: handlers.status_err,
                locale: handlers.locale,
            };
            row_actions.spawn_regenerate_from_user_line(msg_idx, message_id.to_string());
            true
        }
        "branch" => {
            if handlers.stream_turn_busy_ui.get_untracked() {
                return true;
            }
            let row_actions = MessageRowActionSignals {
                chat: handlers.chat,
                stream_follow_up: handlers.stream_follow_up,
                status_err: handlers.status_err,
                locale: handlers.locale,
            };
            row_actions.spawn_branch_at_user_line(msg_idx, message_id.to_string());
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn msg(id: &str, role: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: role.to_string(),
            text: "hi".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn user_menu_has_copy_regen_branch() {
        let keys = turn_menu_action_keys(&msg("u1", "user"));
        assert_eq!(keys, ["copy", "regen", "branch"]);
    }

    #[test]
    fn failed_assistant_menu_has_retry() {
        let mut a = msg("a1", "assistant");
        a.state = Some(StoredMessageState::Error);
        let keys = turn_menu_action_keys(&a);
        assert_eq!(keys, ["copy", "retry"]);
    }

    #[test]
    fn tool_has_no_menu_actions() {
        let mut t = msg("t1", "assistant");
        t.is_tool = true;
        assert!(turn_menu_action_keys(&t).is_empty());
    }

    #[test]
    fn long_assistant_menu_is_copy_only() {
        let long_text = "x".repeat(500);
        let m = StoredMessage {
            id: "a2".into(),
            role: "assistant".into(),
            text: long_text,
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        };
        assert_eq!(turn_menu_action_keys(&m), ["copy"]);
    }
}
