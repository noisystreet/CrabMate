//! `TimelineLog`（`kind: final_response`）与紧随的 **`AssistantAnswerPhase`**：Web 主气泡与时间线契约的共用收尾帧（见 **`docs/SSE_PROTOCOL.md`**）。

use tokio::sync::mpsc::Sender;

use super::{SsePayload, TimelineLogBody, encode_message, send_string_logged};

/// 依次下发 **`final_response`** 时间线与 **`assistant_answer_phase: true`**。
///
/// 与分层总结、`chat_job_queue` 流式兜底等路径对齐；**不**写入 `messages`（由调用方负责）。
pub(crate) async fn send_final_response_timeline_then_answer_phase(
    out: &Sender<String>,
    title: String,
    log_context_timeline: &'static str,
    log_context_phase: &'static str,
) {
    let final_tl = encode_message(SsePayload::TimelineLog {
        log: TimelineLogBody {
            kind: "final_response".to_string(),
            title,
            detail: None,
        },
    });
    let _ = send_string_logged(out, final_tl, log_context_timeline).await;
    let phase_payload = encode_message(SsePayload::AssistantAnswerPhase {
        assistant_answer_phase: true,
    });
    let _ = send_string_logged(out, phase_payload, log_context_phase).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sends_timeline_then_answer_phase_in_order() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
        send_final_response_timeline_then_answer_phase(
            &tx,
            "summary text".to_string(),
            "test::timeline",
            "test::phase",
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
