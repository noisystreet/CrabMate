//! Web 会话快照 JSON（`GET /conversation/messages`）：在 [`Message`] 上附加 `display_*` 字段。

use serde::Serialize;

use crate::cm_runtime::message_display::{
    assistant_markdown_source_for_message, user_message_for_chat_display,
};
use crate::cm_types::{Message, message_content_as_str};

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

/// 过滤后的会话消息转为带 `display_*` 的快照行。
///
/// `role=tool` **不**填 `display_*`：像素级工具卡由 Client `crabmate-tool-card` 在水合时生成
///（见 Client `append_tool_role_timeline_row` 对缺字段的回退）。
pub fn web_client_snapshot_messages(messages: &[Message]) -> Vec<WebClientSnapshotMessage> {
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

/// 默认快照（与 Web 默认语言一致；工具卡文案由 Client 本地化）。
pub fn web_client_snapshot_messages_default_zh(
    messages: &[Message],
) -> Vec<WebClientSnapshotMessage> {
    web_client_snapshot_messages(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::Message;

    #[test]
    fn tool_role_omits_display_fields() {
        let envelope = r#"{"crabmate_tool":{"v":1,"name":"read_file","summary":"读：a.rs","ok":true,"output":"content"}}"#;
        let m = Message {
            role: "tool".into(),
            content: Some(envelope.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("c1".into()),
        };
        let rows = web_client_snapshot_messages(&[m]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].display_content.is_none());
        assert!(rows[0].display_reasoning_content.is_none());
        assert_eq!(
            message_content_as_str(&rows[0].message.content).unwrap(),
            envelope
        );
    }

    #[test]
    fn user_and_assistant_still_get_display_content() {
        let rows = web_client_snapshot_messages(&[
            Message::user_only("hi"),
            Message::assistant_only("hey"),
        ]);
        assert_eq!(rows[0].display_content.as_deref(), Some("hi"));
        assert_eq!(rows[1].display_content.as_deref(), Some("hey"));
    }
}
