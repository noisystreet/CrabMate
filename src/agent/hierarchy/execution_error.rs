//! 分层执行器错误类型（供 `execution.rs` 与 `execution_helpers` 共用）。

use super::operator;

/// 分层执行器错误
#[derive(Debug)]
pub enum ExecutionError {
    DagError(String),
    MaxFailuresReached(String),
    OperatorError(operator::OperatorError),
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::DagError(s) => write!(f, "DAG error: {}", s),
            ExecutionError::MaxFailuresReached(s) => write!(f, "Max failures: {}", s),
            ExecutionError::OperatorError(e) => write!(f, "Operator error: {}", e),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<operator::OperatorError> for ExecutionError {
    fn from(e: operator::OperatorError) -> Self {
        ExecutionError::OperatorError(e)
    }
}
