//! 无 HTTP SSE 通道时（CLI/TUI）镜像 [`super::protocol::SsePayload`]，与 Web `/chat/stream` 控制面语义对齐。
//!
//! 通过 [`SseControlMirror`] 回调复用与 [`super::encode_message`] 相同的负载形状（不经 `data:` 行封装）。

use std::sync::Arc;

use tokio::sync::mpsc::Sender;

use super::protocol::{SsePayload, encode_message};
use super::{send_string_logged, send_string_logged_cooperative_cancel};

/// 与 Web SSE 控制面同形的回合事件回调（`SsePayload` 克隆后投递）。
pub type SseControlMirror = Arc<dyn Fn(SsePayload) + Send + Sync>;

#[inline]
pub fn mirror_sse_control_optional(mirror: Option<&SseControlMirror>, payload: &SsePayload) {
    if let Some(m) = mirror {
        m(payload.clone());
    }
}

/// 先镜像（若有）、再写入 SSE 通道（若有）；与仅 Web 路径相比多支持 **`out == None`** 时仍触发镜像。
pub async fn send_sse_control_payload_optional(
    out: Option<&Sender<String>>,
    mirror: Option<&SseControlMirror>,
    payload: SsePayload,
    context: &'static str,
) -> bool {
    mirror_sse_control_optional(mirror, &payload);
    let Some(tx) = out else {
        return true;
    };
    send_string_logged(tx, encode_message(payload), context).await
}

/// 协作取消变体：发送失败时置位 **`cancel`**（与 [`send_string_logged_cooperative_cancel`] 一致）。
#[allow(dead_code)] // 预留与协作取消路径对齐；当前调用点仍多用 [`send_string_logged_cooperative_cancel`]。
pub async fn send_sse_control_payload_cooperative_cancel_optional(
    out: Option<&Sender<String>>,
    mirror: Option<&SseControlMirror>,
    payload: SsePayload,
    context: &'static str,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> bool {
    mirror_sse_control_optional(mirror, &payload);
    let Some(tx) = out else {
        return true;
    };
    send_string_logged_cooperative_cancel(tx, encode_message(payload), context, cancel).await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc;

    use super::super::protocol::SsePayload;
    use super::*;

    #[tokio::test]
    async fn mirror_optional_invokes_callback_when_some() {
        let seen = Arc::new(Mutex::new(Vec::<SsePayload>::new()));
        let seen_cb = Arc::clone(&seen);
        let mirror: SseControlMirror = Arc::new(move |p| {
            seen_cb.lock().expect("lock").push(p);
        });
        let p = SsePayload::WorkspaceChanged {
            workspace_changed: true,
        };
        mirror_sse_control_optional(Some(&mirror), &p);
        let v = seen.lock().expect("lock");
        assert_eq!(v.len(), 1);
    }

    #[tokio::test]
    async fn send_sse_control_payload_optional_without_tx_still_ok() {
        let p = SsePayload::ToolRunning {
            tool_running: false,
        };
        assert!(send_sse_control_payload_optional(None, None, p, "test_ctx").await);
    }

    #[tokio::test]
    async fn send_sse_control_payload_optional_with_tx_delivers() {
        let (tx, mut rx) = mpsc::channel::<String>(4);
        let p = SsePayload::ParsingToolCalls {
            parsing_tool_calls: true,
        };
        assert!(send_sse_control_payload_optional(Some(&tx), None, p, "test_ctx").await);
        drop(tx);
        let line = rx.recv().await.expect("line");
        assert!(line.contains("parsing_tool_calls"));
    }

    #[tokio::test]
    async fn send_sse_control_cooperative_cancel_sets_flag_when_send_fails() {
        let (tx, rx) = mpsc::channel::<String>(1);
        drop(rx);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let p = SsePayload::AssistantAnswerPhase {
            assistant_answer_phase: true,
        };
        assert!(
            !send_sse_control_payload_cooperative_cancel_optional(
                Some(&tx),
                None,
                p,
                "test_ctx",
                Some(&cancel),
            )
            .await
        );
        assert!(cancel.load(std::sync::atomic::Ordering::SeqCst));
    }
}
