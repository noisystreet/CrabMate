//! Manager 对外类型与 `ManagerAgent` 结构体。

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;

use super::super::session_state::SessionStateManager;
use super::super::task::{ExecutionStrategy, SubGoal};

/// Manager 重规划 / 失败处理等路径共用的 LLM 传输上下文（避免重复传递 cfg/client/key）。
pub struct ManagerLlmContext<'a> {
    pub cfg: &'a AgentConfig,
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
}

/// Manager Agent 配置
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// 最大子目标数量
    pub max_sub_goals: usize,
    /// 执行策略
    pub execution_strategy: ExecutionStrategy,
    /// 是否启用
    pub enabled: bool,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            max_sub_goals: 10,
            execution_strategy: ExecutionStrategy::Hybrid,
            enabled: true,
        }
    }
}

/// Manager Agent 错误
#[derive(Debug)]
pub enum ManagerError {
    LlmError(String),
    ParseError(String),
    ExecutionError(String),
}

/// Manager 对失败子目标的决策
#[derive(Debug)]
pub enum ManagerDecision {
    /// 重新执行修正后的子目标
    Retry { updated_goal: Box<SubGoal> },
    /// 跳过该子目标
    Skip { reason: String },
    /// 终止整个任务
    Abort { reason: String },
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagerError::LlmError(s) => write!(f, "LLM error: {}", s),
            ManagerError::ParseError(s) => write!(f, "Parse error: {}", s),
            ManagerError::ExecutionError(s) => write!(f, "Execution error: {}", s),
        }
    }
}

impl std::error::Error for ManagerError {}

/// Manager Agent
#[derive(Clone)]
pub struct ManagerAgent {
    pub(crate) config: ManagerConfig,
    /// 会话状态管理器（可选）
    pub(crate) session_manager: Option<std::sync::Arc<SessionStateManager>>,
}
