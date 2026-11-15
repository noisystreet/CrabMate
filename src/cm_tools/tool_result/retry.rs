//! 错误码 → 重试范围的**单一**表。
//!
//! 归前有两份白名单，新增瞬时错误码时漏改任一份，该码就会在一侧静默变成「不可重试」：
//!
//! - `tool_result::tool_error_retryable_heuristic`（6 码，喂前端 / 模型提示）
//! - `cm_workflow::execute::retry::workflow_node_failure_retryable`（3 码，编排层自动重跑节点）
//!
//! 两者**不是包含关系**：`workflow_node_missing_result` 值得提示模型重试，但**不该**让编排层
//! 自动重跑节点（缺结果多半不是瞬时故障）。所以用「该码最多能用到哪一档」表达，
//! 读取侧各取所需：`WorkflowNode ⊂ ToolHint`。

/// 重试范围：某个错误码**最多**能支持到哪一档重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryScope {
    /// 业务失败：既不该提示模型重试，也不该自动重跑（重复执行可能有副作用）。
    NotRetryable,
    /// 可提示模型「换个方式再试」；编排层不自动重跑。
    ToolHint,
    /// 编排层可对工作流节点自动重试（无副作用或幂等的瞬时故障）。
    WorkflowNode,
}

/// 单一来源表。新增瞬时错误码**只**改这里，两侧判定自动跟随。
pub(crate) const RETRY_SCOPES: &[(&str, RetryScope)] = &[
    ("timeout", RetryScope::WorkflowNode),
    ("workflow_tool_join_error", RetryScope::WorkflowNode),
    ("workflow_semaphore_closed", RetryScope::WorkflowNode),
    ("http_timeout", RetryScope::ToolHint),
    ("rate_limited", RetryScope::ToolHint),
    ("workflow_node_missing_result", RetryScope::ToolHint),
];

/// 码 → 范围；未登记的码一律 [`RetryScope::NotRetryable`]（保守：不认识就不重试）。
pub fn error_code_retry_scope(error_code: Option<&str>) -> RetryScope {
    let Some(code) = error_code else {
        return RetryScope::NotRetryable;
    };
    RETRY_SCOPES
        .iter()
        .find(|(known, _)| *known == code)
        .map_or(RetryScope::NotRetryable, |(_, scope)| *scope)
}

/// 工具层提示口径（原 `tool_error_retryable_heuristic`）。
pub fn tool_error_retryable(error_code: Option<&str>) -> bool {
    error_code_retry_scope(error_code) != RetryScope::NotRetryable
}

/// 编排层自动重试口径（原 `workflow_node_failure_retryable`）。
pub fn workflow_node_retryable(error_code: Option<&str>) -> bool {
    error_code_retry_scope(error_code) == RetryScope::WorkflowNode
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表内每个码的两个口径都必须与表一致——这是「只写一次」唯一能被验证的方式。
    #[test]
    fn both_scopes_agree_with_the_single_table() {
        for (code, scope) in RETRY_SCOPES {
            assert_eq!(
                tool_error_retryable(Some(code)),
                *scope != RetryScope::NotRetryable,
                "工具层口径与表不一致：{code}"
            );
            assert_eq!(
                workflow_node_retryable(Some(code)),
                *scope == RetryScope::WorkflowNode,
                "编排层口径与表不一致：{code}"
            );
        }
    }

    /// 原两份白名单的集合不能因为归一变宽或变窄。
    #[test]
    fn historical_sets_are_preserved() {
        for code in [
            "timeout",
            "http_timeout",
            "rate_limited",
            "workflow_tool_join_error",
            "workflow_semaphore_closed",
            "workflow_node_missing_result",
        ] {
            assert!(tool_error_retryable(Some(code)), "工具层原本可重试：{code}");
        }
        for code in [
            "timeout",
            "workflow_tool_join_error",
            "workflow_semaphore_closed",
        ] {
            assert!(workflow_node_retryable(Some(code)), "编排层原本可重试：{code}");
        }
        // 编排层原本就**不**重试这个码。
        assert!(tool_error_retryable(Some("workflow_node_missing_result")));
        assert!(!workflow_node_retryable(Some(
            "workflow_node_missing_result"
        )));
    }

    #[test]
    fn unknown_and_missing_codes_are_not_retryable() {
        assert!(!tool_error_retryable(Some("invalid_args")));
        assert!(!workflow_node_retryable(Some("invalid_args")));
        assert!(!tool_error_retryable(None));
        assert!(!workflow_node_retryable(None));
    }
}
