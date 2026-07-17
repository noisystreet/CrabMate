//! V2 编码器：将 `SsePayload` 按 AG-UI 协议序列化为 SSE `data:` 行 JSON 字符串。
//!
//! 每个 `SsePayload` 可输出一个或多个 AG-UI 事件（如 `ToolCall` 拆为 START/ARGS/END），
//! 用 `\n` 分隔。前端 V2Parser 逐行解析。

use super::ag_ui_convert::convert_sse_payload_to_ag_ui;
use super::ag_ui_encode::encode_ag_ui_event;
use super::encoder::SseEncoder;
use super::protocol::SsePayload;

/// v2 编码器：AG-UI 协议格式（`client_sse_protocol=2`）。
pub struct V2Encoder;

impl SseEncoder for V2Encoder {
    fn encode(&self, payload: &SsePayload) -> String {
        let events = convert_sse_payload_to_ag_ui(payload);
        events
            .iter()
            .map(encode_ag_ui_event)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::protocol::{
        SseCapabilitiesBody, SseErrorBody, StreamEndedBody, ToolCallSummary, ToolResultBody,
    };
    use crabmate_sse_protocol::StreamEndReason;

    #[test]
    fn v2_encoder_stream_ended() {
        let encoder = V2Encoder;
        let s = encoder.encode(&SsePayload::StreamEnded {
            ended: StreamEndedBody {
                job_id: 1,
                reason: StreamEndReason::Completed,
                tiktoken_prompt_tokens: None,
            },
        });
        // 应为单行 RUN_FINISHED
        assert!(s.contains(r#""type":"RUN_FINISHED""#), "got: {s}");
        assert!(!s.contains('\n'), "should be single line: {s}");
        // 当前仅 AG-UI（v2），`default_encoder` 返回 V2Encoder
    }

    #[test]
    fn v2_encoder_error() {
        let encoder = V2Encoder;
        let s = encoder.encode(&SsePayload::Error(SseErrorBody {
            error: "fail".into(),
            code: Some("ERR".into()),
            reason_code: None,
            turn_id: None,
            sub_phase: None,
        }));
        assert!(s.contains(r#""type":"RUN_ERROR""#), "got: {s}");
    }

    #[test]
    fn v2_encoder_tool_call_splits_into_three_lines() {
        let encoder = V2Encoder;
        let s = encoder.encode(&SsePayload::ToolCall {
            tool_call: ToolCallSummary {
                name: "read_file".into(),
                summary: "x".into(),
                goal_id: None,
                tool_call_id: Some("tc-1".into()),
                arguments_preview: None,
                arguments: Some(r#"{"path":"/etc/hosts"}"#.into()),
            },
        });
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(
            lines.len(),
            3,
            "ToolCall should produce 3 AG-UI events, got {} lines: {s:?}",
            lines.len()
        );
        assert!(
            lines[0].contains(r#""type":"TOOL_CALL_START""#),
            "line 0: {}",
            lines[0]
        );
        assert!(
            lines[1].contains(r#""type":"TOOL_CALL_ARGS""#),
            "line 1: {}",
            lines[1]
        );
        assert!(
            lines[2].contains(r#""type":"TOOL_CALL_END""#),
            "line 2: {}",
            lines[2]
        );
    }

    #[test]
    fn v2_encoder_custom_events() {
        let encoder = V2Encoder;
        let s = encoder.encode(&SsePayload::SseCapabilities {
            caps: SseCapabilitiesBody {
                supported_sse_v: 2,
                resume_ring_cap: 512,
                job_id: 1,
            },
        });
        assert!(s.contains(r#""type":"CUSTOM""#), "got: {s}");
        assert!(s.contains(r#""customType":"sse_capabilities""#), "got: {s}");
    }

    #[test]
    fn v2_encoder_tool_result() {
        let encoder = V2Encoder;
        let s = encoder.encode(&SsePayload::ToolResult {
            tool_result: ToolResultBody {
                name: "run_command".into(),
                goal_id: None,
                result_version: 1,
                summary: None,
                output: "done".into(),
                ok: Some(true),
                exit_code: Some(0),
                error_code: None,
                failure_category: None,
                retryable: None,
                tool_call_id: Some("tc-1".into()),
                execution_mode: None,
                parallel_batch_id: None,
                stdout: None,
                stderr: None,
                structured_preview: None,
            },
        });
        assert!(s.contains(r#""type":"TOOL_CALL_RESULT""#), "got: {s}");
    }
}
