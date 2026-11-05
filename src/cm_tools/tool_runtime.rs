//! Web 工具运行时上下文（审批通道、白名单）。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::cm_types::CommandApprovalDecision;
use tokio::sync::{Mutex as TokioMutex, mpsc};

/// 单次工具分发的运行时句柄（仅 Web SSE 审批通道；运维 CLI 无同进程对话审批）。
pub struct ToolRuntime<'a> {
    pub workspace_changed: &'a mut bool,
    /// 审批通道（P3，S3-a）：只依赖 `crate::cm_approval::ApprovalBroker` 端口，
    /// 不再写进具体实现类型 `WebToolRuntime`。
    pub ctx: Option<&'a dyn crate::cm_approval::ApprovalBroker>,
}

pub struct WebToolRuntime {
    pub out_tx: mpsc::Sender<String>,
    pub approval_rx_shared: Arc<TokioMutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<TokioMutex<()>>,
    pub persistent_allowlist_shared: Arc<TokioMutex<HashSet<String>>>,
    /// 当前回合的流任务 `job_id`（`x-stream-job-id` / `sse_capabilities.job_id`）。
    ///
    /// 后台任务（`async=true`）发起时写入 `JobRecord.source_turn_job_id`，供
    /// `POST /chat/stream/{job_id}/cancel` 级联取消本回合尚未终态的后台任务。
    /// 运维 CLI 等无同进程 SSE 的路径为 `None`。
    pub turn_job_id: Option<u64>,
    /// 当前回合的协作式取消标志；交互审批等待据此在用户点「停止」后提前退出。
    pub cancel: Option<Arc<AtomicBool>>,
}

impl crate::cm_approval::ApprovalBroker for WebToolRuntime {
    fn approval_sink(&self) -> Option<crate::cm_approval::WebApprovalSink<'_>> {
        Some(crate::cm_approval::WebApprovalSink {
            out_tx: &self.out_tx,
            approval_rx_shared: &self.approval_rx_shared,
            approval_request_guard: &self.approval_request_guard,
            cancel: self.cancel.as_ref(),
        })
    }

    fn turn_job_id(&self) -> Option<u64> {
        self.turn_job_id
    }

    fn out_tx(&self) -> &mpsc::Sender<String> {
        &self.out_tx
    }

    fn persistent_allowlist(&self) -> &Arc<TokioMutex<HashSet<String>>> {
        &self.persistent_allowlist_shared
    }
}
