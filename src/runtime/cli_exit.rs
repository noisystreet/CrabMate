//! `chat` 子命令等 CLI 场景的**退出码**与可 downcast 的错误类型（供 `main` 映射 `std::process::exit`）。

/// 一般失败（配置、I/O、未分类错误）
pub const EXIT_GENERAL: i32 = 1;
/// 参数/用法错误（与 clap 解析失败区分：运行时发现输入不合法）
pub const EXIT_USAGE: i32 = 2;
/// 模型接口或解析失败（非配额类 HTTP 状态）
pub const EXIT_MODEL_ERROR: i32 = 3;
/// 本回合内所有 `run_command` 调用均被用户拒绝（批处理/脚本中用于检测「命令全拒」）
pub const EXIT_TOOLS_ALL_RUN_COMMAND_DENIED: i32 = 4;
/// 配额 / 限流（典型 HTTP 429；部分网关亦可能用 503）
pub const EXIT_QUOTA_OR_RATE_LIMIT: i32 = 5;
/// `tool-replay run --compare-recorded` 下存在与录制不一致的工具输出
pub const EXIT_TOOL_REPLAY_MISMATCH: i32 = 6;

#[derive(Debug, Clone)]
pub struct CliExitError {
    pub code: i32,
    pub message: String,
}

impl std::fmt::Display for CliExitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliExitError {}

impl CliExitError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 根据 `run_agent_turn` / LLM 层常见错误文案归类退出码（启发式，与 `llm/api.rs` 用户可见串对齐）。
pub fn classify_model_error_message(msg: &str) -> i32 {
    if msg.contains("HTTP 429")
        || msg.contains("http 429")
        || msg.contains("status=429")
        || msg.contains("限流")
        || msg.contains("quota")
        || msg.contains("Quota")
    {
        return EXIT_QUOTA_OR_RATE_LIMIT;
    }
    if msg.contains("HTTP 402")
        || msg.contains("http 402")
        || msg.contains("余额")
        || msg.contains("insufficient")
    {
        return EXIT_QUOTA_OR_RATE_LIMIT;
    }
    if msg.contains("HTTP 503") || msg.contains("http 503") || msg.contains("status=503") {
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
