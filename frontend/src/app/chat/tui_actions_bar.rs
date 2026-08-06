//! 终端流回合操作：可用动作列表与点击分发（UI 为右键 / 长按菜单，见 `message_turn_menu`）。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::composer_follow_up::ComposerStreamFollowUp;
use super::message_row_actions::MessageRowActionSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::write_clipboard_text;
use crate::storage::StoredMessage;
use crate::stream_text_overlay::message_text_for_display_including_stream_overlay;

/// 窄屏长 assistant 消息默认折叠阈值（与 `mobile.css` 渐变遮罩配合）。
pub(crate) const LONG_ASSISTANT_COLLAPSE_CHARS: usize = 480;

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

fn is_long_collapsible_assistant(message: &StoredMessage) -> bool {
    message.role == "assistant"
        && !message.is_tool
        && !message
            .state
            .as_ref()
            .is_some_and(crate::storage::StoredMessageState::is_loading)
        && message.text.chars().count() >= LONG_ASSISTANT_COLLAPSE_CHARS
}

/// 供 [`super::tui_transcript_sync`] 在 section 上附加 `chat-tui-turn--long`。
#[must_use]
pub(crate) fn long_assistant_turn_class_suffix(message: &StoredMessage) -> &'static str {
    if is_long_collapsible_assistant(message) {
        " chat-tui-turn--long"
    } else {
        ""
    }
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
    if is_long_collapsible_assistant(message) {
        keys.push("toggle-expand");
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

fn toggle_long_message_expanded(message_id: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let selector = format!("section.chat-tui-turn[data-tui-msg-id=\"{message_id}\"]");
    let Ok(Some(section)) = doc.query_selector(selector.as_str()) else {
        return;
    };
    let class_list = section.class_list();
    if class_list.contains("chat-tui-turn--expanded") {
        let _ = class_list.remove_1("chat-tui-turn--expanded");
    } else {
        let _ = class_list.add_1("chat-tui-turn--expanded");
    }
}

/// 点击折叠中的长消息时展开（仅展开，不切换收起；收起仍走右键/长按菜单）。
pub(crate) fn expand_collapsed_long_turn_on_click(ev: &web_sys::MouseEvent) {
    let Some(target) = ev.target() else {
        return;
    };
    // 少数引擎可能把 target 落在 #text：升到父 Element 再 closest。
    let Some(el) = target.dyn_ref::<web_sys::Element>().cloned().or_else(|| {
        target
            .dyn_ref::<web_sys::Node>()
            .and_then(|n| n.parent_element())
    }) else {
        return;
    };
    // 链接 / 按钮 / 工具 details 等保持原交互，不抢展开。
    if el
        .closest("a, button, summary, input, textarea, select, label, .session-ctx-layer")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    // 正在选中文本时不展开，避免拖选结束后误触。
    if web_sys::window()
        .and_then(|w| w.get_selection().ok().flatten())
        .is_some_and(|s| !s.is_collapsed() && !String::from(s.to_string()).trim().is_empty())
    {
        return;
    }
    let Ok(Some(section)) = el.closest("section.chat-tui-turn.chat-tui-turn--long") else {
        return;
    };
    let class_list = section.class_list();
    if class_list.contains("chat-tui-turn--expanded") || class_list.contains("is-loading") {
        return;
    }
    let _ = class_list.add_1("chat-tui-turn--expanded");
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
        "toggle-expand" => {
            toggle_long_message_expanded(message_id);
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
    fn long_assistant_gets_toggle_expand() {
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
        let keys = turn_menu_action_keys(&m);
        assert!(keys.contains(&"toggle-expand"), "{keys:?}");
        assert_eq!(long_assistant_turn_class_suffix(&m), " chat-tui-turn--long");
    }
}
