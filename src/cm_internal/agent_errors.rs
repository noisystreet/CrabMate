//! Web / CLI 共用的 `run_agent_turn` 与 LLM 错误串**启发式**分类（避免多处子串判断漂移）。

use crate::cm_types::LLM_CANCELLED_ERROR;

/// 与协作取消等路径返回的 [`LLM_CANCELLED_ERROR`] 对齐。
pub fn is_user_cancelled_run_agent_error(s: &str) -> bool {
    s.trim() == LLM_CANCELLED_ERROR
}

const QUOTA_OR_RATE_LIMIT_NEEDLES: &[&str] = &[
    "HTTP 429",
    "http 429",
    "status=429",
    "限流",
    "quota",
    "Quota",
    "HTTP 402",
    "http 402",
    "余额",
    "insufficient",
    "HTTP 503",
    "http 503",
    "status=503",
];

/// 配额 / 限流 / 余额类（与 `llm::api` 常见中文文案及 HTTP 状态片段对齐）。
pub fn is_quota_or_rate_limit_llm_message(msg: &str) -> bool {
    QUOTA_OR_RATE_LIMIT_NEEDLES.iter().any(|n| msg.contains(n))
}

/// 把 LLM / `run_agent_turn` 错误文案归类为 CLI 退出码（启发式，与 `llm::api` 用户可见串对齐）。
pub fn classify_model_error_message(msg: &str) -> i32 {
    use crate::cm_runtime::cli_exit::{EXIT_GENERAL, EXIT_MODEL_ERROR, EXIT_QUOTA_OR_RATE_LIMIT};

    if is_quota_or_rate_limit_llm_message(msg) {
        return EXIT_QUOTA_OR_RATE_LIMIT;
    }
    if msg.contains("模型接口返回错误")
        || msg.contains("模型返回内容无法解析")
        || msg.contains("非流式响应 choices 为空")
        || msg.contains("无 tool_calls")
    {
        return EXIT_MODEL_ERROR;
    }
    EXIT_GENERAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::LLM_CANCELLED_ERROR;

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

    #[test]
    fn classify_model_error_maps_429_to_quota() {
        assert_eq!(
            classify_model_error_message("模型接口返回错误（HTTP 429）：too many requests"),
            crate::cm_runtime::cli_exit::EXIT_QUOTA_OR_RATE_LIMIT
        );
    }

    #[test]
    fn classify_model_error_maps_generic_model_line() {
        let got = classify_model_error_message("模型接口返回错误（HTTP 401）：bad key");
        assert_eq!(got, crate::cm_runtime::cli_exit::EXIT_MODEL_ERROR);
    }
}
