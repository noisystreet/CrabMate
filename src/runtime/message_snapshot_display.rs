//! Web 会话快照 JSON（`GET /conversation/messages`）：在 [`Message`] 上附加 `display_*` 字段。

use serde::Serialize;

use crate::runtime::message_display::{
    assistant_markdown_source_for_message, tool_content_for_display_for_message,
    user_message_for_chat_display,
};
use crate::types::{Message, message_content_as_str};

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

/// 过滤后的会话消息转为带 `display_*` 的快照行（模型上下文仍用 `message.content`）。
pub(crate) fn web_client_snapshot_messages(messages: &[Message]) -> Vec<WebClientSnapshotMessage> {
    messages
        .iter()
        .enumerate()
        .map(|(idx, m)| {
            let mut display_content = None;
            let mut display_reasoning_content = None;
            let raw = message_content_as_str(&m.content).unwrap_or("").to_string();
            match m.role.as_str() {
                "user" => {
                    display_content = Some(user_message_for_chat_display(&raw));
                }
                "tool" => {
                    let detail = tool_content_for_display_for_message(&raw, messages, idx);
                    display_reasoning_content = Some(detail.clone());
                    display_content = Some(compact_tool_display_line(&detail));
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

fn compact_tool_display_line(detail: &str) -> String {
    const MAX: usize = 180;
    let compact = detail
        .split_whitespace()
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= MAX {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(MAX.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}
