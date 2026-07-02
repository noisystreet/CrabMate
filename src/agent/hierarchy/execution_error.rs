//! 分层执行器错误类型（供 `execution.rs` 与 `execution_helpers` 共用）。

use super::operator;
use super::turn_abort::HierarchicalTurnAbortReason;

/// 分层执行器错误
#[derive(Debug)]
pub enum ExecutionError {
    DagError(String),
    MaxFailuresReached(String),
    /// 执行器未注入必需上下文（如未调用 `with_context`）。
    MissingContext(String),
    OperatorError(operator::OperatorError),
    /// 用户取消或 SSE 断开（与主 Agent 外循环早停语义对齐）。
    TurnAborted(HierarchicalTurnAbortReason),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::DagError(s) => write!(f, "DAG error: {}", s),
            ExecutionError::MaxFailuresReached(s) => write!(f, "Max failures: {}", s),
            ExecutionError::MissingContext(s) => write!(f, "Missing executor context: {}", s),
            ExecutionError::OperatorError(e) => write!(f, "Operator error: {}", e),
            ExecutionError::TurnAborted(r) => write!(f, "{}", r.user_message()),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<operator::OperatorError> for ExecutionError {
    fn from(e: operator::OperatorError) -> Self {
        ExecutionError::OperatorError(e)
    }
}
