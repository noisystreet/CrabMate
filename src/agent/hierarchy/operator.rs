//! Operator Agent：执行子目标的 ReAct 循环
//!
//! Operator 负责：
//! - 理解子目标
//! - 决定工具调用
//! - 执行 ReAct 循环

use super::task::{Capability, SubGoal, TaskResult, TaskStatus};
use log::info;
use std::time::Instant;

/// Operator Agent 配置
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// 最大 ReAct 迭代次数
    pub max_iterations: usize,
    /// 可用的工具列表（为空表示使用全部工具）
    pub allowed_tools: Vec<String>,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            allowed_tools: Vec::new(),
        }
    }
}

/// Operator Agent 错误
#[derive(Debug)]
pub enum OperatorError {
    MaxIterationsReached,
    ToolNotAllowed(String),
}

impl std::fmt::Display for OperatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperatorError::MaxIterationsReached => write!(f, "Max iterations reached"),
            OperatorError::ToolNotAllowed(t) => write!(f, "Tool not allowed: {}", t),
        }
    }
}

impl std::error::Error for OperatorError {}

/// Operator Agent
pub struct OperatorAgent {
    config: OperatorConfig,
}

impl OperatorAgent {
    pub fn new(config: OperatorConfig) -> Self {
        Self { config }
    }

    /// 执行子目标（简化版本）
    /// 完整版本需要调用 LLM 进行 ReAct 循环
    pub async fn execute(&self, goal: &SubGoal) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        info!(target: "crabmate", "Operator executing goal: {}", goal.goal_id);

        // 简化版本：直接标记为完成
        // 完整版本需要实现 ReAct 循环
        Ok(TaskResult {
            task_id: goal.goal_id.clone(),
            status: TaskStatus::Completed,
            output: Some(format!("Completed: {}", goal.description)),
            error: None,
            artifacts: Vec::new(),
            duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// 检查工具是否允许
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.config.allowed_tools.is_empty()
            || self.config.allowed_tools.contains(&tool_name.to_string())
    }
}

/// 根据能力获取工具列表
pub fn get_tools_for_capabilities(capabilities: &[Capability]) -> Vec<String> {
    let mut tool_names = Vec::new();

    for cap in capabilities {
        match cap {
            Capability::FileRead => {
                tool_names.push("read_file".to_string());
                tool_names.push("glob".to_string());
                tool_names.push("grep".to_string());
            }
            Capability::FileWrite => {
                tool_names.push("write_file".to_string());
            }
            Capability::CommandExecution => {
                tool_names.push("run_command".to_string());
            }
            Capability::NetworkRequest => {
                tool_names.push("http_fetch".to_string());
            }
            Capability::WebSearch => {
                // 暂不实现
            }
        }
    }

    // 去重
    tool_names.sort();
    tool_names.dedup();
    tool_names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute() {
        let config = OperatorConfig::default();
        let operator = OperatorAgent::new(config);
        let goal = SubGoal::new("test", "测试目标").with_capabilities(vec![Capability::FileRead]);

        let result = operator.execute(&goal).await.unwrap();
        assert!(matches!(result.status, TaskStatus::Completed));
    }

    #[test]
    fn test_get_tools_for_capabilities() {
        let caps = vec![Capability::FileRead, Capability::CommandExecution];
        let tools = get_tools_for_capabilities(&caps);
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"run_command".to_string()));
    }
}
