//! CLI 退出码与 `CliExitError` 类型（与 `classify_model_error_message` 分离，
//! 后者因依赖根包 `agent_errors` / `plan_artifact` 保留在根包）。

/// 一般失败（配置、I/O、未分类错误）
pub const EXIT_GENERAL: i32 = 1;
/// 参数/用法错误
pub const EXIT_USAGE: i32 = 2;
/// 模型接口或解析失败
pub const EXIT_MODEL_ERROR: i32 = 3;
/// 本回合内所有 `run_command` 调用均被用户拒绝
pub const EXIT_TOOLS_ALL_RUN_COMMAND_DENIED: i32 = 4;
/// 配额 / 限流
pub const EXIT_QUOTA_OR_RATE_LIMIT: i32 = 5;
/// `tool-replay` 下存在不一致的工具输出
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
