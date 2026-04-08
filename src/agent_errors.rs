//! Web / CLI 共用的 `run_agent_turn` 与 LLM 错误串**启发式**分类（避免多处子串判断漂移）。

use crate::types::LLM_CANCELLED_ERROR;

/// 与协作取消等路径返回的 [`LLM_CANCELLED_ERROR`] 对齐。
pub(crate) fn is_user_cancelled_run_agent_error(s: &str) -> bool {
    s.trim() == LLM_CANCELLED_ERROR
}

/// 配额 / 限流 / 余额类（与 `llm::api` 常见中文文案及 HTTP 状态片段对齐）。
pub(crate) fn is_quota_or_rate_limit_llm_message(msg: &str) -> bool {
    msg.contains("HTTP 429")
        || msg.contains("http 429")
        || msg.contains("status=429")
        || msg.contains("限流")
        || msg.contains("quota")
        || msg.contains("Quota")
        || msg.contains("HTTP 402")
        || msg.contains("http 402")
        || msg.contains("余额")
        || msg.contains("insufficient")
        || msg.contains("HTTP 503")
        || msg.contains("http 503")
        || msg.contains("status=503")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LLM_CANCELLED_ERROR;

    #[test]
    fn user_cancelled_trim_matches() {
        assert!(is_user_cancelled_run_agent_error(LLM_CANCELLED_ERROR));
        assert!(is_user_cancelled_run_agent_error(&format!(
            "  {}  ",
            LLM_CANCELLED_ERROR
        )));
    }

    #[test]
    fn quota_heuristic_429() {
        assert!(is_quota_or_rate_limit_llm_message(
            "模型接口返回错误（HTTP 429）：x"
        ));
    }
}
