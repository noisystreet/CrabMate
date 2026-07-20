//! 重导出自 `crabmate-runtime` + `classify_model_error_message`（依赖根包 `agent_errors` / `plan_artifact`）。

pub use crabmate_runtime::cli_exit::*;

/// 根据 `run_agent_turn` / LLM 层常见错误文案归类退出码（启发式，与 `llm::api` 用户可见串对齐）。
pub fn classify_model_error_message(msg: &str) -> i32 {
    if crate::agent_errors::is_quota_or_rate_limit_llm_message(msg) {
        return EXIT_QUOTA_OR_RATE_LIMIT;
    }
    if msg.contains("模型接口返回错误")
        || msg.contains("模型返回内容无法解析")
        || msg.contains("非流式响应 choices 为空")
        || msg.contains("无 tool_calls")
        || msg.starts_with(
            crate::agent::plan_artifact::STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX,
        )
    {
        return EXIT_MODEL_ERROR;
    }
    EXIT_GENERAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_429() {
        assert_eq!(
            classify_model_error_message("模型接口返回错误（HTTP 429）：too many requests"),
            EXIT_QUOTA_OR_RATE_LIMIT
        );
    }

    #[test]
    fn classify_generic_model_line() {
        assert_eq!(
            classify_model_error_message("模型接口返回错误（HTTP 401）：bad key"),
            EXIT_MODEL_ERROR
        );
    }

    #[test]
    fn classify_staged_plan_invalid_run_agent_turn_error() {
        let msg = format!(
            "{} not_found",
            crate::agent::plan_artifact::STAGED_PLAN_INVALID_RUN_AGENT_TURN_ERROR_PREFIX
        );
        assert_eq!(classify_model_error_message(&msg), EXIT_MODEL_ERROR);
    }
}
