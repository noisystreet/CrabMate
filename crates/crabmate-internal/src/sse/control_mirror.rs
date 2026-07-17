//! 无 HTTP SSE 通道时（CLI/TUI）镜像 [`super::protocol::SsePayload`]，与 Web `/chat/stream` 控制面语义对齐。
//!
//! 通过 [`SseControlMirror`] 回调复用与 [`super::encode_message`] 相同的负载形状（不经 `data:` 行封装）。

use std::sync::Arc;

use tokio::sync::mpsc::Sender;

use super::encoder::SseEncoder;
use super::protocol::SsePayload;
use super::send_string_logged;

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
    encoder: &dyn SseEncoder,
) -> bool {
    mirror_sse_control_optional(mirror, &payload);
    let Some(tx) = out else {
        return true;
    };
    send_string_logged(tx, encoder.encode(&payload), context).await
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
        let encoder = crate::sse::V2Encoder;
        let p = SsePayload::ToolRunning {
            tool_running: false,
        };
        assert!(send_sse_control_payload_optional(None, None, p, "test_ctx", &encoder).await);
    }

    #[tokio::test]
    async fn send_sse_control_payload_optional_with_tx_delivers() {
        let encoder = crate::sse::V2Encoder;
        let (tx, mut rx) = mpsc::channel::<String>(4);
        let p = SsePayload::ParsingToolCalls {
            parsing_tool_calls: true,
        };
        assert!(send_sse_control_payload_optional(Some(&tx), None, p, "test_ctx", &encoder).await);
        drop(tx);
        let line = rx.recv().await.expect("line");
        assert!(line.contains("parsing_tool_calls"));
    }
}
