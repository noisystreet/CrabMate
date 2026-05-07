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
