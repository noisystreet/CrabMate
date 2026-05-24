//! Web 会话快照 JSON（`GET /conversation/messages`）：在 [`Message`] 上附加 `display_*` 字段。

use serde::Serialize;

use crate::runtime::message_display::{
    assistant_markdown_source_for_message, user_message_for_chat_display,
};
use crate::tool_result::normalize_tool_message_content;
use crate::types::{Message, message_content_as_str};
use crabmate_tool_card::{NormalizedToolSnapshotFields, ToolCardLocale, tool_stored_text};

/// 客户端快照单条消息：`Message` 字段 + 可选展示层正文。
#[derive(Debug, Clone, Serialize)]
pub struct WebClientSnapshotMessage {
    #[serde(flatten)]
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_reasoning_content: Option<String>,
}

/// 过滤后的会话消息转为带 `display_*` 的快照行（与 Web SSE/水合 [`tool_stored_text`] 同源）。
pub(crate) fn web_client_snapshot_messages(
    messages: &[Message],
    locale: ToolCardLocale,
) -> Vec<WebClientSnapshotMessage> {
    messages
        .iter()
        .map(|m| {
            let mut display_content = None;
            let mut display_reasoning_content = None;
            let raw = message_content_as_str(&m.content).unwrap_or("").to_string();
            match m.role.as_str() {
                "user" => {
                    display_content = Some(user_message_for_chat_display(&raw));
                }
                "tool" => {
                    if let Some(env) = normalize_tool_message_content(&raw) {
                        let input = crabmate_tool_card::ToolCardInput::from_normalized_fields(
                            NormalizedToolSnapshotFields {
                                name: env.name,
                                summary: env.summary,
                                output: env.output,
                                ok: env.ok,
                                exit_code: env.exit_code,
                                error_code: env.error_code,
                                failure_category: env.failure_category,
                                tool_call_id: env.tool_call_id,
                                structured_payload: env.structured_payload,
                            },
                        );
                        let stored = tool_stored_text(&input, locale);
                        display_content = Some(stored.compact);
                        display_reasoning_content = Some(stored.detail);
                    }
                }
                "assistant" => {
                    display_content = Some(assistant_markdown_source_for_message(m));
                    let reasoning = m.reasoning_content.as_deref().unwrap_or("").trim();
                    if !reasoning.is_empty() {
                        display_reasoning_content = Some(reasoning.to_string());
                    }
                }
                _ => {}
            }
            WebClientSnapshotMessage {
                message: m.clone(),
                display_content,
                display_reasoning_content,
            }
        })
        .collect()
}

/// 默认 zh-Hans 快照（与 Web 默认语言一致）。
pub(crate) fn web_client_snapshot_messages_default_zh(
    messages: &[Message],
) -> Vec<WebClientSnapshotMessage> {
    web_client_snapshot_messages(messages, ToolCardLocale::ZhHans)
}
