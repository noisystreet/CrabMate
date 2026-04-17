//! Manager Agent：任务分解与协调
//!
//! Manager 负责：
//! - 理解高层任务目标
//! - 分解为可执行的 SubGoals
//! - 确定执行策略
//! - 协调子目标执行

use super::task::{Capability, ExecutionStrategy, SubGoal, TaskResult};

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
    ParseError(String),
    ExecutionError(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagerError::ParseError(s) => write!(f, "Parse error: {}", s),
            ManagerError::ExecutionError(s) => write!(f, "Execution error: {}", s),
        }
    }
}

impl std::error::Error for ManagerError {}

/// Manager Agent
pub struct ManagerAgent {
    config: ManagerConfig,
}

impl ManagerAgent {
    pub fn new(config: ManagerConfig) -> Self {
        Self { config }
    }

    /// 分解任务为子目标（简化版本，需要外部 LLM 调用）
    /// 返回 ManagerOutput 包含分解结果
    pub async fn decompose(&self, task: &str) -> Result<ManagerOutput, ManagerError> {
        // 简化版本：直接返回单目标
        // 完整版本需要调用 LLM 进行分解
        let sub_goals = vec![
            SubGoal::new("goal_1", task)
                .with_capabilities(vec![Capability::FileRead, Capability::CommandExecution]),
        ];

        Ok(ManagerOutput {
            sub_goals,
            execution_strategy: self.config.execution_strategy,
            summary: String::new(),
        })
    }

    /// 获取执行策略
    pub fn execution_strategy(&self) -> ExecutionStrategy {
        self.config.execution_strategy
    }
}

/// Manager 输出
#[derive(Debug, Clone)]
pub struct ManagerOutput {
    /// 分解的子目标列表
    pub sub_goals: Vec<SubGoal>,
    /// 执行策略
    pub execution_strategy: ExecutionStrategy,
    /// 给用户的结果摘要
    pub summary: String,
}

/// 失败处理决策
#[derive(Debug, Clone)]
pub enum FailureDecision {
    Continue,
    Retry { goal_id: String },
    Skip { goal_id: String, reason: String },
    Replan { reason: String },
    Abort { reason: String },
}

/// 处理失败
pub fn handle_failure(
    results: &[TaskResult],
    max_failures: usize,
) -> (Vec<&TaskResult>, Vec<&TaskResult>, FailureDecision) {
    let completed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, super::task::TaskStatus::Completed))
        .collect();

    let failed: Vec<&TaskResult> = results
        .iter()
        .filter(|r| matches!(r.status, super::task::TaskStatus::Failed { .. }))
        .collect();

    let decision = if failed.is_empty() {
        FailureDecision::Continue
    } else if failed.len() > max_failures {
        FailureDecision::Abort {
            reason: format!("{} failures exceeded threshold", failed.len()),
        }
    } else {
        FailureDecision::Continue
    };

    (completed, failed, decision)
}

#[cfg(test)]
mod tests {
    use super::super::task::TaskStatus;
    use super::*;

    #[tokio::test]
    async fn test_decompose() {
        let manager = ManagerAgent::new(ManagerConfig::default());
        let output = manager.decompose("测试任务").await.unwrap();
        assert_eq!(output.sub_goals.len(), 1);
    }

    #[test]
    fn test_handle_failure() {
        let results = vec![
            TaskResult {
                task_id: "1".to_string(),
                status: TaskStatus::Completed,
                output: None,
                error: None,
                artifacts: vec![],
                duration_ms: 0,
            },
            TaskResult {
                task_id: "2".to_string(),
                status: TaskStatus::Failed {
                    reason: "error".to_string(),
                },
                output: None,
                error: None,
                artifacts: vec![],
                duration_ms: 0,
            },
        ];

        let (completed, failed, decision) = handle_failure(&results, 0);
        assert_eq!(completed.len(), 1);
        assert_eq!(failed.len(), 1);
        assert!(matches!(decision, FailureDecision::Abort { .. }));
    }
}
