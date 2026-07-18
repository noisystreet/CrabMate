//! AG-UI 事件序列化：将 `AgUiEvent` 编码为 SSE `data:` 行 JSON 字符串。
//!
//! 输出格式遵循 AG-UI 协议：`{"type":"EVENT_NAME","field1":"val1",...}`

use super::ag_ui_event::AgUiEvent;

/// 将 AG-UI 事件序列化为单行 JSON 字符串。
pub(crate) fn encode_ag_ui_event(event: &AgUiEvent) -> String {
    // 使用 #[serde(tag = "type")] 自动生成 type 字段
    serde_json::to_string(event).unwrap_or_else(|e| {
        log::error!(
            target: "crabmate",
            "ag_ui encode failed error={}",
            e
        );
        // 兜底：序列化失败时输出一个可解析的错误事件
        r#"{"type":"RUN_ERROR","threadId":"","runId":"","error":{"message":"内部AG-UI序列化失败","code":"AG_UI_ENCODE"}}"#.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::ag_ui_event::AgUiErrorBody;

    #[test]
    fn text_message_content_roundtrip() {
        let event = AgUiEvent::TextMessageContent {
            message_id: "msg-1".into(),
            delta: "hello".into(),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"TEXT_MESSAGE_CONTENT""#), "got: {s}");
        assert!(s.contains(r#""messageId":"msg-1""#), "got: {s}");
        assert!(s.contains(r#""delta":"hello""#), "got: {s}");
    }

    #[test]
    fn tool_call_start_roundtrip() {
        let event = AgUiEvent::ToolCallStart {
            tool_call_id: "tc-1".into(),
            name: "read_file".into(),
            parent_message_id: "msg-1".into(),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"TOOL_CALL_START""#), "got: {s}");
        assert!(s.contains(r#""toolCallId":"tc-1""#), "got: {s}");
        assert!(s.contains(r#""name":"read_file""#), "got: {s}");
    }

    #[test]
    fn run_started_contains_type() {
        let event = AgUiEvent::RunStarted {
            thread_id: "th-1".into(),
            run_id: "run-1".into(),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"RUN_STARTED""#), "got: {s}");
    }

    #[test]
    fn run_error_contains_error_field() {
        let event = AgUiEvent::RunError {
            thread_id: "th-1".into(),
            run_id: "run-1".into(),
            error: AgUiErrorBody {
                message: "something went wrong".into(),
                code: Some("ERR_1".into()),
            },
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"RUN_ERROR""#), "got: {s}");
        assert!(
            s.contains(r#""message":"something went wrong""#),
            "got: {s}"
        );
        assert!(s.contains(r#""code":"ERR_1""#), "got: {s}");
    }

    #[test]
    fn custom_event_preserves_type_and_data() {
        let event = AgUiEvent::Custom {
            custom_type: "intent_analysis".into(),
            data: serde_json::json!({"intent": "execute.code_change", "confidence": 0.95}),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"CUSTOM""#), "got: {s}");
        assert!(s.contains(r#""customType":"intent_analysis""#), "got: {s}");
        assert!(s.contains(r#""intent":"execute.code_change""#), "got: {s}");
    }

    #[test]
    fn reasoning_message_roundtrip() {
        let event = AgUiEvent::ReasoningMessageContent {
            message_id: "reason-1".into(),
            delta: "thinking step 1".into(),
        };
        let s = encode_ag_ui_event(&event);
        assert!(
            s.contains(r#""type":"REASONING_MESSAGE_CONTENT""#),
            "got: {s}"
        );
        assert!(s.contains(r#""messageId":"reason-1""#), "got: {s}");
    }

    #[test]
    fn state_snapshot_roundtrip() {
        let event = AgUiEvent::StateSnapshot {
            state: serde_json::json!({"turn": "completed"}),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""type":"STATE_SNAPSHOT""#), "got: {s}");
        assert!(s.contains(r#""turn":"completed""#), "got: {s}");
    }

    #[test]
    fn metadata_is_skipped_when_none() {
        let event = AgUiEvent::TextMessageStart {
            message_id: "msg-1".into(),
            role: "assistant".into(),
            metadata: None,
        };
        let s = encode_ag_ui_event(&event);
        assert!(!s.contains("metadata"), "metadata unexpected in: {s}");
    }

    #[test]
    fn metadata_included_when_some() {
        let event = AgUiEvent::TextMessageStart {
            message_id: "msg-1".into(),
            role: "assistant".into(),
            metadata: Some(serde_json::json!({"kind": "batch_narration"})),
        };
        let s = encode_ag_ui_event(&event);
        assert!(s.contains(r#""metadata""#), "got: {s}");
        assert!(s.contains(r#""kind":"batch_narration""#), "got: {s}");
    }
}
