//! Web SSE 桥接用 `tokio::sync::mpsc::Sender<String>`：发送失败时打 **debug**，便于排查客户端断连、receiver 提前 drop。
//!
//! 流式对话路径在提供 **`cancel`** 时应用 [`send_string_logged_cooperative_cancel`]：发送失败则置位 **`AtomicBool`**，使 [`crate::llm::api::stream_chat`] 尽快结束上游读取；队列侧在任务取消且通道仍可投递时补发 **`STREAM_CANCELLED`**（见 **`docs/SSE_PROTOCOL.md`**）。

use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::Sender;

/// 发送一行 SSE 负载；失败时记录 **debug**（通道已关闭或无 receiver）。
///
/// 返回 `true` 表示已入队；`false` 表示未送达（通常可配合 `tx.is_closed()` 与上层早退逻辑）。
#[inline]
pub async fn send_string_logged(tx: &Sender<String>, line: String, context: &'static str) -> bool {
    match tx.send(line).await {
        Ok(()) => true,
        Err(_e) => {
            debug!(
                target: "crabmate::sse_mpsc",
                "SSE mpsc String send failed context={} (receiver dropped or channel closed)",
                context
            );
            false
        }
    }
}

/// 同 [`send_string_logged`]；若 **`cancel`** 为 `Some` 且发送失败，则 **`store(true, SeqCst)`**（仅当此前为 `false` 时打 **info**），供 `llm::api::stream_chat` 与队列 **`cancel`** 标志对齐，尽快跳出 SSE 消费循环。
#[inline]
pub async fn send_string_logged_cooperative_cancel(
    tx: &Sender<String>,
    line: String,
    context: &'static str,
    cancel: Option<&AtomicBool>,
) -> bool {
    match tx.send(line).await {
        Ok(()) => true,
        Err(_e) => {
            debug!(
                target: "crabmate::sse_mpsc",
                "SSE mpsc String send failed context={} (receiver dropped or channel closed)",
                context
            );
            if let Some(c) = cancel {
                let prev = c.swap(true, Ordering::SeqCst);
                if !prev {
                    info!(
                        target: "crabmate::sse_mpsc",
                        "SSE 发送失败，已置协作取消标志 context={}",
                        context
                    );
                }
            }
            false
        }
    }
}
