//! `TimelineLog`（`kind: final_response`）与紧随的 **`AssistantAnswerPhase`**：Web 主气泡与时间线契约的共用收尾帧（见 **`docs/SSE协议.md`**）。
//!
//! 另有 AG-UI `STATE_SNAPSHOT` 辅助函数。

use tokio::sync::mpsc::Sender;

use super::ag_ui_encode::encode_ag_ui_event;
use super::ag_ui_event::AgUiEvent;
use super::encoder::SseEncoder;
use super::{SsePayload, TimelineLogBody, send_string_logged};

/// 若使用 V2 编码器（AG-UI 协议），在工具批结束等边界发送 `STATE_SNAPSHOT`。
///
/// V1 编码器下此函数为空操作（V1 前端不识别 STATE_SNAPSHOT）。
pub async fn send_state_snapshot_sse(
    out: &Sender<String>,
    state: serde_json::Value,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::StateSnapshot { state };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::state_snapshot").await;
}

/// 发送 AG-UI `TEXT_MESSAGE_START`（仅 V2 下生效）。
pub async fn send_text_message_start_sse(
    out: &Sender<String>,
    message_id: &str,
    role: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::TextMessageStart {
        message_id: message_id.to_string(),
        role: role.to_string(),
        metadata: None,
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::text_message_start").await;
}

/// 发送 AG-UI `TEXT_MESSAGE_END`（仅 V2 下生效）。
pub async fn send_text_message_end_sse(
    out: &Sender<String>,
    message_id: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::TextMessageEnd {
        message_id: message_id.to_string(),
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::text_message_end").await;
}

/// 发送 AG-UI `REASONING_MESSAGE_START`（仅 V2 下生效）。
pub async fn send_reasoning_message_start_sse(
    out: &Sender<String>,
    message_id: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::ReasoningMessageStart {
        message_id: message_id.to_string(),
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::reasoning_message_start").await;
}

/// 发送 AG-UI `REASONING_MESSAGE_CONTENT`（仅 V2 下生效）。
pub async fn send_reasoning_message_content_sse(
    out: &Sender<String>,
    message_id: &str,
    delta: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::ReasoningMessageContent {
        message_id: message_id.to_string(),
        delta: delta.to_string(),
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::reasoning_message_content").await;
}

/// 发送 AG-UI `REASONING_MESSAGE_END`（仅 V2 下生效）。
pub async fn send_reasoning_message_end_sse(
    out: &Sender<String>,
    message_id: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::ReasoningMessageEnd {
        message_id: message_id.to_string(),
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::reasoning_message_end").await;
}

/// 发送 AG-UI `RUN_STARTED`（仅 V2 下生效）。
pub async fn send_run_started_sse(
    out: &Sender<String>,
    thread_id: &str,
    run_id: &str,
    encoder: &dyn SseEncoder,
) {
    if encoder.format_version() != 2 {
        return;
    }
    let event = AgUiEvent::RunStarted {
        thread_id: thread_id.to_string(),
        run_id: run_id.to_string(),
    };
    let line = encode_ag_ui_event(&event);
    let _ = send_string_logged(out, line, "sse::run_started").await;
}

/// 编码 AG-UI `TEXT_MESSAGE_CONTENT`。返回编码后的 JSON 字符串；V1 下返回原始 delta。
/// 主要用于流式文本增量通道（stream_host_impl 等内部编码器）。
pub fn encode_text_message_content_sse(delta: &str, encoder: &dyn SseEncoder) -> String {
    if encoder.format_version() != 2 {
        return delta.to_string();
    }
    let event = AgUiEvent::TextMessageContent {
        message_id: "msg-assistant-turn".to_string(),
        delta: delta.to_string(),
    };
    encode_ag_ui_event(&event)
}

/// 编码 AG-UI `REASONING_MESSAGE_CONTENT`。返回编码后的 JSON 字符串；V1 下返回原始 delta。
pub fn encode_reasoning_message_content_sse(delta: &str, encoder: &dyn SseEncoder) -> String {
    if encoder.format_version() != 2 {
        return delta.to_string();
    }
    let event = AgUiEvent::ReasoningMessageContent {
        message_id: "reasoning".to_string(),
        delta: delta.to_string(),
    };
    encode_ag_ui_event(&event)
}

/// 编码 AG-UI `TEXT_MESSAGE_START`。返回编码后的 JSON 字符串；V1 下返回空字符串。
pub fn encode_text_message_start_sse_str(encoder: &dyn SseEncoder) -> String {
    if encoder.format_version() != 2 {
        return String::new();
    }
    let event = AgUiEvent::TextMessageStart {
        message_id: "msg-assistant-turn".to_string(),
        role: "assistant".to_string(),
        metadata: None,
    };
    encode_ag_ui_event(&event)
}

/// 依次下发 **`final_response`** 时间线与 **`assistant_answer_phase: true`**。
///
/// 与分层总结、`chat_job_queue` 流式兜底等路径对齐；**不**写入 `messages`（由调用方负责）。
pub async fn send_final_response_timeline_then_answer_phase(
    out: &Sender<String>,
    title: String,
    log_context_timeline: &'static str,
    log_context_phase: &'static str,
    encoder: &dyn SseEncoder,
) {
    let final_tl = encoder.encode(&SsePayload::TimelineLog {
        log: TimelineLogBody {
            kind: "final_response".to_string(),
            title,
            detail: None,
        },
    });
    let _ = send_string_logged(out, final_tl, log_context_timeline).await;
    let phase_payload = encoder.encode(&SsePayload::AssistantAnswerPhase {
        assistant_answer_phase: true,
    });
    let _ = send_string_logged(out, phase_payload, log_context_phase).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sends_timeline_then_answer_phase_in_order() {
        let encoder = crate::sse::V1Encoder;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
        send_final_response_timeline_then_answer_phase(
            &tx,
            "summary text".to_string(),
            "test::timeline",
            "test::phase",
            &encoder,
        )
        .await;
        let first = rx.recv().await.expect("timeline frame");
        let second = rx.recv().await.expect("answer phase frame");
        assert!(
            first.contains("final_response") && first.contains("summary text"),
            "first frame should be final_response timeline: {first}"
        );
        assert!(
            second.contains("assistant_answer_phase"),
            "second frame should be answer phase: {second}"
        );
    }
}
