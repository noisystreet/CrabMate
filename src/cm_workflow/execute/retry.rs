//! 节点失败是否可自动重试（超时 / join / 信号量等，避免对业务失败重复执行有副作用工具）。
//!
//! 口径**单一来源**：[`crate::cm_tool_result::retry::RETRY_SCOPES`] 那张表。
//! 归前本文件与 `tool_result::tool_error_retryable_heuristic` 各有一份白名单（3 码 / 6 码），
//! 新增瞬时错误码时漏改任一份，该码就会在一侧静默变成「不可重试」（#1401 P1）。
//!
//! 注意两侧**不是包含关系**：`workflow_node_missing_result` 值得提示模型重试，
//! 但不该让编排层自动重跑节点，所以表里它只是 `RetryScope::ToolHint`。

/// 是否对节点失败做**自动重试**（保守：避免对业务失败重复执行有副作用工具）。
pub(crate) fn workflow_node_failure_retryable(error_code: Option<&str>) -> bool {
    crate::cm_tool_result::retry::workflow_node_retryable(error_code)
}

#[cfg(test)]
mod tests {
    use super::workflow_node_failure_retryable;
    use crate::cm_tool_result::retry::{RETRY_SCOPES, RetryScope};

    /// 与 `cm_tools` 共用同一张表：新增/改动码时两侧不可能各自漂移。
    #[test]
    fn workflow_scope_follows_the_single_table() {
        for (code, scope) in RETRY_SCOPES {
            assert_eq!(
                workflow_node_failure_retryable(Some(code)),
                *scope == RetryScope::WorkflowNode,
                "编排层口径与单一表不一致：{code}"
            );
        }
    }

    /// 原白名单口径不能因为归一变宽：`workflow_node_missing_result` 仍不可自动重跑。
    #[test]
    fn missing_result_code_stays_non_retryable_for_nodes() {
        assert!(!workflow_node_failure_retryable(Some(
            "workflow_node_missing_result"
        )));
        assert!(workflow_node_failure_retryable(Some("timeout")));
        assert!(!workflow_node_failure_retryable(Some("invalid_args")));
        assert!(!workflow_node_failure_retryable(None));
    }
}
