//! TUI 风格纯文本聊天视图：复用会话与 SSE overlay，只替换消息展示层。

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::storage::StoredMessage;
use crate::stream_text_overlay::{
    StreamTextOverlay, message_text_for_display_including_stream_overlay,
};

fn tui_role_label(message: &StoredMessage, locale: Locale) -> String {
    if message.is_tool {
        return message
            .tool_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map_or_else(
                || i18n::msg_role_tool(locale).to_string(),
                |name| format!("{}:{name}", i18n::msg_role_tool(locale)),
            );
    }
    match message.role.as_str() {
        "user" => i18n::msg_role_user(locale),
        "assistant" => i18n::msg_role_assistant(locale),
        "system" => i18n::msg_role_system(locale),
        _ => i18n::msg_role_other(locale),
    }
    .to_string()
}

fn build_tui_transcript(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    let mut transcript = String::new();
    for message in messages {
        if !transcript.is_empty() {
            transcript.push_str("\n\n");
        }
        transcript.push_str(&tui_role_label(message, locale));
        transcript.push_str(" ❯\n");
        transcript.push_str(&message_text_for_display_including_stream_overlay(
            message,
            overlay,
            session_id,
            locale,
            apply_assistant_display_filters,
        ));
    }
    transcript
}

#[component]
pub(crate) fn ChatTuiStreamView(
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
) -> impl IntoView {
    let transcript = move || {
        let _ = chat.stream_overlay_revision.get();
        let active_id = chat.active_id.get();
        let locale = locale.get();
        let apply_filters = apply_assistant_display_filters.get();
        let overlay = chat.stream_text_overlay.get();
        chat.sessions.with(|sessions| {
            sessions
                .iter()
                .find(|session| session.id == active_id)
                .map_or_else(
                    || i18n::chat_tui_empty(locale).to_string(),
                    |session| {
                        if session.messages.is_empty() {
                            i18n::chat_tui_empty(locale).to_string()
                        } else {
                            build_tui_transcript(
                                &session.messages,
                                &session.id,
                                overlay.as_ref(),
                                locale,
                                apply_filters,
                            )
                        }
                    },
                )
        })
    };

    view! {
        <div
            class="messages-inner chat-tui-inner"
            data-testid="chat-tui-stream-view"
        >
            <pre
                class="chat-tui-transcript"
                data-testid="chat-tui-transcript"
                aria-live="polite"
                aria-atomic="false"
            >{transcript}</pre>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn message(id: &str, role: &str, text: &str) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
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
    fn transcript_keeps_markdown_as_plain_text() {
        let messages = vec![
            message("u1", "user", "你好"),
            message("a1", "assistant", "**原样**"),
        ];
        let output = build_tui_transcript(&messages, "s1", None, Locale::ZhHans, false);
        assert_eq!(output, "用户 ❯\n你好\n\n助手 ❯\n**原样**");
    }

    #[test]
    fn transcript_includes_live_stream_overlay() {
        let mut assistant = message("a1", "assistant", "");
        assistant.state = Some(StoredMessageState::Loading);
        let overlay = StreamTextOverlay {
            session_id: "s1".to_string(),
            message_id: "a1".to_string(),
            answer: "流式片段".to_string(),
            reasoning: String::new(),
        };
        let output =
            build_tui_transcript(&[assistant], "s1", Some(&overlay), Locale::ZhHans, false);
        assert_eq!(output, "助手 ❯\n流式片段");
    }
}
