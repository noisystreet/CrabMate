//! `run_stream_queued_job` 的 SSE 桥接与执行上下文准备。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{debug, info, warn};
use tokio::sync::mpsc;

use super::super::{WebApprovalSession, WebChatJobEnvelope, WebChatQueueDeps};
use super::prelude::{PreparedQueuedTurn, PreparedTurnParams, prepare_queued_turn};

pub(super) struct StreamJobRuntime {
    pub sse_tx: mpsc::Sender<String>,
    pub cancel: Arc<AtomicBool>,
    /// 回合前置解析结果（配置 / 角色 / 工具 / 权限 / executor / per-flight），与 JSON 路径共用。
    /// 持有它即持有 `flight` 强引用 ⇒ 本任务在整个回合期间都在 `/status` 的 `per_active_jobs` 里。
    pub prepared: PreparedQueuedTurn,
    pub web_tool_ctx: Option<crate::tool_registry::WebToolRuntime>,
    pub approval_session_id: Option<String>,
}

pub(super) struct StreamJobSetupParams<'a> {
    pub envelope: &'a WebChatJobEnvelope,
    pub stream_event_tx: mpsc::Sender<(u64, String)>,
    pub web_approval_session: Option<WebApprovalSession>,
    pub queue_deps: &'a WebChatQueueDeps,
    /// 与 [`ChatJobQueue::register_stream_cancel`] 同一 `Arc`（入队时已创建）。
    pub cancel: Arc<AtomicBool>,
}

pub(super) async fn stream_job_setup_runtime(
    p: StreamJobSetupParams<'_>,
) -> (StreamJobRuntime, tokio::task::JoinHandle<()>) {
    let job_id = p.envelope.job_id;
    let http_tx = p.stream_event_tx.clone();
    let (sse_tx, sse_rx) = mpsc::channel::<String>(1024);
    // 投递句柄随桥接任务走：它是登记项唯一的强引用，任务退出（通道关闭 ⟺ 不可能再产帧）
    // 即摘除登记，worker 侧因此不再需要持有它（#1316）。
    tokio::spawn(stream_job_bridge_loop(
        sse_rx,
        p.queue_deps.sse_stream_hub.register_job(job_id),
        http_tx,
    ));

    let caps_line = crate::sse::encode_message(crate::sse::SsePayload::SseCapabilities {
        caps: crate::sse::SseCapabilitiesBody {
            supported_sse_v: crate::sse::protocol::SSE_PROTOCOL_VERSION,
            resume_ring_cap: crate::sse::protocol::SSE_RESUME_RING_CAP,
            job_id,
        },
    });
    let _ = crate::sse::send_string_logged(
        &sse_tx,
        caps_line,
        "chat_job_queue::stream sse_capabilities",
    )
    .await;

    // 标记回合开始与思维链生命周期
    crate::sse::send_run_started_sse(
        &sse_tx,
        "main",              // thread_id
        &job_id.to_string(), // run_id
    )
    .await;
    crate::sse::send_reasoning_message_start_sse(&sse_tx, "reasoning").await;

    let cancel = p.cancel;
    let (web_tool_ctx, approval_session_id) =
        stream_job_web_tool_ctx(p.web_approval_session, &sse_tx, job_id, Arc::clone(&cancel));

    let cancel_watcher =
        stream_job_spawn_cancel_watcher(sse_tx.clone(), Arc::clone(&cancel), job_id);

    // 前置解析与 JSON 路径共用同一份；`flight` 的强引用随 `prepared` 交给 runtime，
    // 因此 stream 任务在整个回合期间都在 `/status` 的 `per_active_jobs` 里（#1380）。
    let prepared = prepare_queued_turn(PreparedTurnParams {
        envelope: p.envelope,
        queue_deps: p.queue_deps,
    })
    .await;

    let runtime = StreamJobRuntime {
        sse_tx,
        cancel,
        prepared,
        web_tool_ctx,
        approval_session_id,
    };
    (runtime, cancel_watcher)
}

/// SSE 桥接：内部 `mpsc` → hub 投递 → HTTP 侧 `(seq, data)`。
///
/// 用 [`crate::sse::StreamPublisher`] 而不是 `SseStreamHub` + `job_id` 查表：worker 收尾
/// （`drop(sse_tx)` → `retire` → 返回）与桥接任务被调度之间存在窗口，查表投递恰好在这段
/// 窗口里把落盘后的终态帧丢掉（#1316）。
///
/// `sse_rx` 关闭 ⟺ 所有发送端已 drop ⟺ 本流不可能再产帧，所以登记的摘除放在循环之后，
/// 而不是由 worker 猜一个时刻。
pub(crate) async fn stream_job_bridge_loop(
    mut sse_rx: mpsc::Receiver<String>,
    publisher: crate::sse::StreamPublisher,
    http_tx: mpsc::Sender<(u64, String)>,
) {
    while let Some(line) = sse_rx.recv().await {
        // 客户端已断开时不要提前 break：环形缓冲还要继续累积，供 `stream_resume` 重放。
        // E4/S1（#1338）：终态后的回合业务帧由 `publish` 直接拒绝（返回 `None`），
        // 此时既不写 HTTP 流，也不留序号空洞。
        if let Some(ev) = publisher.publish(line) {
            let _ = http_tx.send(ev).await;
        }
    }
    log_terminal_order_summary(&publisher);
    publisher.finalize();
}

/// 本流的终态序汇总（#1333 P1 / Turn §16.6）。
///
/// 违规在 [`crate::sse::StreamPublisher::publish`] 里已按帧 `warn` 过；这里只补一条**收尾**
/// 记录：关闭 B4 要求 `event_after_terminal_count == 0` 连续一个版本区间，光看「没有 warn」
/// 分不清「干净跑完」与「流压根没跑到终态就断了」，所以要连 `terminal_seen` 一起记。
fn log_terminal_order_summary(publisher: &crate::sse::StreamPublisher) {
    let report = publisher.terminal_order_report();
    if report.is_clean() {
        debug!(
            target: "crabmate",
            "terminal_order_summary job_id={} event_after_terminal_count=0 repeated_terminal_count=0 error_then_finished_count={} terminal_seen={}",
            publisher.job_id(),
            report.error_then_finished,
            report.terminal_seen
        );
    } else {
        warn!(
            target: "crabmate",
            "terminal_order_summary job_id={} event_after_terminal_count={} repeated_terminal_count={} error_then_finished_count={} terminal_seen={}",
            publisher.job_id(),
            report.event_after_terminal,
            report.repeated_terminal,
            report.error_then_finished,
            report.terminal_seen
        );
    }
}

fn stream_job_web_tool_ctx(
    web_approval_session: Option<WebApprovalSession>,
    sse_tx: &mpsc::Sender<String>,
    job_id: u64,
    cancel: Arc<AtomicBool>,
) -> (Option<crate::tool_registry::WebToolRuntime>, Option<String>) {
    if let Some(session) = web_approval_session {
        (
            Some(crate::tool_registry::WebToolRuntime {
                out_tx: sse_tx.clone(),
                approval_rx_shared: Arc::new(tokio::sync::Mutex::new(session.approval_rx)),
                approval_request_guard: Arc::new(tokio::sync::Mutex::new(())),
                persistent_allowlist_shared: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
                turn_job_id: Some(job_id),
                cancel: Some(cancel),
            }),
            Some(session.session_id),
        )
    } else {
        (None, None)
    }
}

fn stream_job_spawn_cancel_watcher(
    sse_tx: mpsc::Sender<String>,
    cancel: Arc<AtomicBool>,
    job_id: u64,
) -> tokio::task::JoinHandle<()> {
    // 仅当**内部** SSE mpsc 的接收端被 drop 时触发。HTTP 客户端 abort 不会 drop 该接收端
    //（桥接任务仍读 `sse_rx` 以便 `stream_resume`）。用户点「停止」须走
    // `POST /chat/stream/{job_id}/cancel` 置同一 `cancel` 标志。
    tokio::spawn(async move {
        sse_tx.closed().await;
        cancel.store(true, Ordering::SeqCst);
        info!(
            target: "crabmate",
            "chat stream SSE 接收端关闭，已请求取消 job_id={}",
            job_id
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    /// #1316 回归：worker 收尾（入队终态帧 → drop 发送端 → `retire`）**先于**桥接任务首次被
    /// 调度时，已入队帧仍须到达 HTTP 侧。
    ///
    /// 构造不依赖调度细节：桥接任务在 `retire` 之后才 `spawn`，且此前已用 `has_job` 把登记判成
    /// 「不可重连」并顺手摘除 map 键——它的第一次 `poll` 必然晚于摘除时刻，正是线上「落盘后
    /// 连发终态帧、中间没有真实 IO 让出点」的形状。任何退回查表投递的改动都会在这里超时失败。
    #[tokio::test]
    async fn bridge_delivers_terminal_frames_after_registration_is_gone() {
        let hub = crate::sse::SseStreamHub::new();
        let publisher = hub.register_job(9);
        let (http_tx, mut http_rx) = mpsc::channel::<(u64, String)>(8);
        let (sse_tx, sse_rx) = mpsc::channel::<String>(8);

        for frame in ["stream_draining", "conversation_saved", "RUN_FINISHED"] {
            assert!(sse_tx.send(frame.to_string()).await.is_ok(), "入队 {frame}");
        }
        drop(sse_tx);
        // worker 收尾只做 drop 发送端；登记由桥接任务在通道关闭后摘除，这里手动摘除
        // 模拟「登记已经不在」的最坏时点。
        hub.remove_job(9);
        assert!(!hub.has_job(9), "登记已摘除");

        let bridge = tokio::spawn(stream_job_bridge_loop(sse_rx, publisher.clone(), http_tx));

        for want in ["stream_draining", "conversation_saved", "RUN_FINISHED"] {
            let (_, got) = timeout(Duration::from_millis(500), http_rx.recv())
                .await
                .expect("桥接任务应把终态帧送达 HTTP 侧")
                .expect("http 通道不应提前关闭");
            assert_eq!(got, want);
        }
        timeout(Duration::from_millis(500), bridge)
            .await
            .expect("桥接任务应在通道关闭后退出")
            .expect("桥接任务不应 panic");
        assert!(!hub.has_job(9), "桥接任务退出后登记仍应为空");
    }
}
