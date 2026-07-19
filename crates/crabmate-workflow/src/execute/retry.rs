//! 节点失败是否可自动重试（超时 / join / 信号量等，避免对业务失败重复执行有副作用工具）。

/// 是否对节点失败做**自动重试**（保守：避免对业务失败重复执行有副作用工具）。
pub(crate) fn workflow_node_failure_retryable(error_code: Option<&str>) -> bool {
    matches!(
        error_code,
        Some("timeout") | Some("workflow_tool_join_error") | Some("workflow_semaphore_closed")
    )
}
