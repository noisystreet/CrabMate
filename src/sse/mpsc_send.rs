//! Web SSE 桥接用 `tokio::sync::mpsc::Sender<String>`：发送失败时打 **debug**，便于排查客户端断连、receiver 提前 drop（见 `docs/TODOLIST.md` P2）。

use log::debug;
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
