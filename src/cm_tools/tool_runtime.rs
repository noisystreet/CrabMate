//! Web 工具运行时上下文（审批通道、白名单）。

use std::collections::HashSet;
use std::sync::Arc;

use crate::cm_types::CommandApprovalDecision;
use tokio::sync::{Mutex as TokioMutex, mpsc};

/// 单次工具分发的运行时句柄（仅 Web SSE 审批通道；运维 CLI 无同进程对话审批）。
pub struct ToolRuntime<'a> {
    pub workspace_changed: &'a mut bool,
    pub ctx: Option<&'a WebToolRuntime>,
}

pub struct WebToolRuntime {
    pub out_tx: mpsc::Sender<String>,
    pub approval_rx_shared: Arc<TokioMutex<mpsc::Receiver<CommandApprovalDecision>>>,
    pub approval_request_guard: Arc<TokioMutex<()>>,
    pub persistent_allowlist_shared: Arc<TokioMutex<HashSet<String>>>,
}
