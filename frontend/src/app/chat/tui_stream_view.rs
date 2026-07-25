//! TUI 风格聊天视图：复用会话与 SSE overlay；正文按行轻量 Markdown 渲染。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::scroll_follow::follow_after_content_paint;
use super::scroll_shell::ChatScrollShellSignals;
use super::tui_line_markdown::render_tui_line_markdown;
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::markdown::plaintext_to_safe_html;
use crate::storage::{StoredMessage, StoredMessageState};
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

fn message_finalize_open_line(message: &StoredMessage) -> bool {
    !message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
}

fn build_tui_transcript_html(
    messages: &[StoredMessage],
    session_id: &str,
    overlay: Option<&StreamTextOverlay>,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    let mut html = String::new();
    for message in messages {
        let body = message_text_for_display_including_stream_overlay(
            message,
            overlay,
            session_id,
            locale,
            apply_assistant_display_filters,
        );
        let role = tui_role_label(message, locale);
        html.push_str("<section class=\"chat-tui-turn\">");
        html.push_str("<div class=\"chat-tui-role\">");
        html.push_str(&plaintext_to_safe_html(&format!("{role} ❯")));
        html.push_str("</div>");
        html.push_str("<div class=\"chat-tui-body\">");
        html.push_str(&render_tui_line_markdown(
            &body,
            message_finalize_open_line(message),
        ));
        html.push_str("</div></section>");
    }
    html
}

#[component]
pub(crate) fn ChatTuiStreamView(
    chat: ChatSessionSignals,
    locale: RwSignal<Locale>,
    apply_assistant_display_filters: RwSignal<bool>,
    scroll_shell: ChatScrollShellSignals,
) -> impl IntoView {
    let transcript_ref = NodeRef::<leptos::html::Div>::new();

    Effect::new(move |_| {
        let _ = chat.stream_overlay_revision.get();
        let active_id = chat.active_id.get();
        let locale = locale.get();
        let apply_filters = apply_assistant_display_filters.get();
        let overlay = chat.stream_text_overlay.get();
        let html = chat.sessions.with(|sessions| {
            sessions
                .iter()
                .find(|session| session.id == active_id)
                .map_or_else(
                    || {
                        format!(
                            "<div class=\"chat-tui-empty\">{}</div>",
                            plaintext_to_safe_html(i18n::chat_tui_empty(locale))
                        )
                    },
                    |session| {
                        if session.messages.is_empty() {
                            format!(
                                "<div class=\"chat-tui-empty\">{}</div>",
                                plaintext_to_safe_html(i18n::chat_tui_empty(locale))
                            )
                        } else {
                            build_tui_transcript_html(
                                &session.messages,
                                &session.id,
                                overlay.as_ref(),
                                locale,
                                apply_filters,
                            )
                        }
                    },
                )
        });
        if let Some(node) = transcript_ref.get()
            && let Some(el) = node.dyn_ref::<web_sys::HtmlElement>()
        {
            el.set_inner_html(&html);
            // 与 ResizeObserver 互补：innerHTML 增高后立刻若仍 pin 则贴底。
            follow_after_content_paint(scroll_shell);
        }
    });

    view! {
        <div
            class="messages-inner chat-tui-inner"
            data-testid="chat-tui-stream-view"
        >
            <div
                class="chat-tui-transcript"
                data-testid="chat-tui-transcript"
                node_ref=transcript_ref
                aria-live="polite"
                aria-atomic="false"
            />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn finished_assistant_bold_becomes_strong() {
        let messages = vec![
            message("u1", "user", "你好"),
            message("a1", "assistant", "**原样**"),
        ];
        let output = build_tui_transcript_html(&messages, "s1", None, Locale::ZhHans, false);
        assert!(output.contains("用户"), "got {output}");
        assert!(
            output.contains("<strong>") || output.contains("<b>"),
            "got {output}"
        );
        assert!(!output.contains("**原样**"), "got {output}");
    }

    #[test]
    fn loading_open_line_keeps_raw_markers() {
        let mut assistant = message("a1", "assistant", "");
        assistant.state = Some(StoredMessageState::Loading);
        let overlay = StreamTextOverlay {
            session_id: "s1".to_string(),
            message_id: "a1".to_string(),
            answer: "**流式".to_string(),
            reasoning: String::new(),
        };
        let output =
            build_tui_transcript_html(&[assistant], "s1", Some(&overlay), Locale::ZhHans, false);
        assert!(output.contains("**流式"), "got {output}");
        assert!(!output.contains("<strong>"), "got {output}");
    }
}
