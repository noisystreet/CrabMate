//! 终端流每回合下方操作条 HTML（对齐气泡 `msg-actions-below`）与点击分发。

use leptos::prelude::*;

use super::composer_follow_up::ComposerStreamFollowUp;
use super::message_row_actions::MessageRowActionSignals;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::markdown::plaintext_to_safe_html;
use crate::session_ops::write_clipboard_text;
use crate::storage::StoredMessage;
use crate::stream_text_overlay::message_text_for_display_including_stream_overlay;

/// 窄屏长 assistant 消息默认折叠阈值（与 `mobile.css` 渐变遮罩配合）。
pub(crate) const LONG_ASSISTANT_COLLAPSE_CHARS: usize = 480;

/// 是否渲染该回合下方操作条（工具卡无；用户/助手有复制等）。
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

fn svg_chevron_expand() -> &'static str {
    r#"<svg class="msg-action-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>"#
}

fn svg_copy() -> &'static str {
    r#"<svg class="msg-action-icon" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" stroke="currentColor" stroke-width="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" stroke="currentColor" stroke-width="2"/></svg>"#
}

fn svg_regen_retry() -> &'static str {
    r#"<svg class="msg-action-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/></svg>"#
}

fn svg_branch() -> &'static str {
    r#"<svg class="msg-action-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><line x1="6" y1="3" x2="6" y2="15" fill="none"/><circle cx="6" cy="3" r="2" fill="none"/><path d="M6 15v-1a4 4 0 0 1 4-4h4a4 4 0 0 0 4-4V5" fill="none"/><circle cx="18" cy="5" r="2" fill="none"/><circle cx="18" cy="19" r="2" fill="none"/><path d="M18 7v12" fill="none"/></svg>"#
}

fn action_icon_btn(
    action: &str,
    msg_id: &str,
    msg_idx: usize,
    title: &str,
    btn_class: &str,
    icon_html: &str,
) -> String {
    format!(
        "<button type=\"button\" class=\"{btn_class}\" \
         data-testid=\"chat-tui-action-{action}\" data-tui-action=\"{action}\" \
         data-tui-msg-id=\"{id}\" data-tui-msg-idx=\"{msg_idx}\" \
         title=\"{title}\" aria-label=\"{title}\">{icon_html}</button>",
        id = plaintext_to_safe_html(msg_id),
        title = plaintext_to_safe_html(title),
    )
}

/// 单回合下方操作条 HTML；工具或不可见时返回空串。
#[must_use]
pub(crate) fn turn_actions_bar_html(
    message: &StoredMessage,
    msg_idx: usize,
    locale: Locale,
) -> String {
    if !turn_actions_visible(message) {
        return String::new();
    }
    let id = message.id.as_str();
    let muted = "btn btn-muted btn-sm msg-action-btn msg-action-icon-btn";
    let secondary = "btn btn-secondary btn-sm msg-action-icon-btn";
    let mut buttons = String::new();
    buttons.push_str(&action_icon_btn(
        "copy",
        id,
        msg_idx,
        i18n::msg_copy_title(locale),
        muted,
        svg_copy(),
    ));
    if is_user_plain(message) {
        buttons.push_str(&action_icon_btn(
            "regen",
            id,
            msg_idx,
            i18n::msg_regen_title(locale),
            muted,
            svg_regen_retry(),
        ));
        buttons.push_str(&action_icon_btn(
            "branch",
            id,
            msg_idx,
            i18n::msg_branch_title(locale),
            muted,
            svg_branch(),
        ));
    }
    if is_failed_assistant(message) {
        buttons.push_str(&action_icon_btn(
            "retry",
            id,
            msg_idx,
            i18n::msg_retry_title(locale),
            secondary,
            svg_regen_retry(),
        ));
    }
    if is_long_collapsible_assistant(message) {
        buttons.push_str(&action_icon_btn(
            "toggle-expand",
            id,
            msg_idx,
            i18n::msg_toggle_expand_title(locale),
            secondary,
            svg_chevron_expand(),
        ));
    }
    format!(
        "<div class=\"msg-actions msg-actions-below chat-tui-turn-actions\" \
         data-testid=\"chat-tui-turn-actions\" data-tui-actions-for=\"{id}\" \
         role=\"group\" aria-label=\"{aria}\">{buttons}</div>",
        id = plaintext_to_safe_html(id),
        aria = plaintext_to_safe_html(i18n::msg_actions_group_aria(locale)),
    )
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

/// 处理 `data-tui-action` 按钮点击；返回是否已消费事件。
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
    fn user_actions_include_copy_regen_branch_icons() {
        let html = turn_actions_bar_html(&msg("u1", "user"), 0, Locale::ZhHans);
        assert!(html.contains("data-tui-action=\"copy\""), "{html}");
        assert!(html.contains("data-tui-action=\"regen\""), "{html}");
        assert!(html.contains("data-tui-action=\"branch\""), "{html}");
        assert!(!html.contains("data-tui-action=\"retry\""), "{html}");
        assert!(html.contains("msg-action-icon-btn"), "{html}");
        assert!(html.contains("class=\"msg-action-icon\""), "{html}");
    }

    #[test]
    fn failed_assistant_includes_retry_icon() {
        let mut a = msg("a1", "assistant");
        a.state = Some(StoredMessageState::Error);
        let html = turn_actions_bar_html(&a, 1, Locale::ZhHans);
        assert!(html.contains("data-tui-action=\"retry\""), "{html}");
        assert!(html.contains("data-tui-action=\"copy\""), "{html}");
        assert!(html.contains("msg-action-icon-btn"), "{html}");
        assert!(!html.contains("data-tui-action=\"regen\""), "{html}");
    }

    #[test]
    fn tool_has_no_actions() {
        let mut t = msg("t1", "assistant");
        t.is_tool = true;
        assert!(turn_actions_bar_html(&t, 0, Locale::ZhHans).is_empty());
    }

    #[test]
    fn long_assistant_gets_toggle_expand_action() {
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
        let html = turn_actions_bar_html(&m, 0, Locale::ZhHans);
        assert!(html.contains("data-tui-action=\"toggle-expand\""), "{html}");
        assert_eq!(long_assistant_turn_class_suffix(&m), " chat-tui-turn--long");
    }
}
