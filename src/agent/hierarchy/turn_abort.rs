//! 分层执行与用户取消 / SSE 断开对齐（与 `agent_turn::outer_loop` 语义一致）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc::Sender;

/// 分层路径早停原因（映射至 [`crate::agent::agent_turn::errors::TurnAbortReason`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchicalTurnAbortReason {
    UserCancelled,
    SseDisconnected,
}

impl HierarchicalTurnAbortReason {
    pub(crate) fn user_message(self) -> String {
        match self {
            Self::UserCancelled => crate::types::LLM_CANCELLED_ERROR.to_string(),
            Self::SseDisconnected => "流式输出已断开".to_string(),
        }
    }
}

/// 若应中止分层执行则返回原因（先检查用户取消，再检查 SSE 发送端）。
pub(crate) fn hierarchical_abort_reason(
    sse_out: Option<&Sender<String>>,
    cancel: Option<&AtomicBool>,
) -> Option<HierarchicalTurnAbortReason> {
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        return Some(HierarchicalTurnAbortReason::UserCancelled);
    }
    if sse_out.is_some_and(|tx| tx.is_closed()) {
        return Some(HierarchicalTurnAbortReason::SseDisconnected);
    }
    None
}

/// 与 [`hierarchical_abort_reason`] 相同，但 `cancel` 为可在 `spawn` 间共享的 [`Arc`]。
pub(crate) fn hierarchical_abort_reason_arc(
    sse_out: Option<&Sender<String>>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Option<HierarchicalTurnAbortReason> {
    hierarchical_abort_reason(sse_out, cancel.map(|c| c.as_ref()))
}
