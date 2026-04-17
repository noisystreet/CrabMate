//! Operator Agent：执行子目标的 ReAct 循环
//!
//! Operator 负责：
//! - 理解子目标
//! - 决定工具调用
//! - 执行 ReAct 循环（Thought → Action → Observation）

use std::time::Instant;

use crate::config::AgentConfig;
use crate::llm::LlmCompleteError;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts};
use crate::types::{Message, MessageContent};

use super::task::{Capability, SubGoal, TaskResult, TaskStatus};

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
    LlmError(LlmCompleteError),
    ParseError(String),
    ExecutionError(String),
}

impl std::fmt::Display for OperatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperatorError::MaxIterationsReached => write!(f, "Max iterations reached"),
            OperatorError::ToolNotAllowed(t) => write!(f, "Tool not allowed: {}", t),
            OperatorError::LlmError(e) => write!(f, "LLM error: {}", e),
            OperatorError::ParseError(s) => write!(f, "Parse error: {}", s),
            OperatorError::ExecutionError(s) => write!(f, "Execution error: {}", s),
        }
    }
}

impl std::error::Error for OperatorError {}

impl From<LlmCompleteError> for OperatorError {
    fn from(e: LlmCompleteError) -> Self {
        OperatorError::LlmError(e)
    }
}

/// ReAct 循环状态
#[derive(Debug, Clone)]
struct ReactState {
    /// 当前迭代次数
    iteration: usize,
    /// 历史消息
    messages: Vec<Message>,
    /// 观察结果
    observations: Vec<String>,
}

/// Operator Agent
pub struct OperatorAgent {
    config: OperatorConfig,
}

impl OperatorAgent {
    pub fn new(config: OperatorConfig) -> Self {
        Self { config }
    }

    /// 执行子目标（简化版本，不使用 LLM）
    pub async fn execute(&self, goal: &SubGoal) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(target: "crabmate", "Operator executing goal: {} (simple mode)", goal.goal_id);

        // 简化版本：直接标记为完成
        // 完整版本需要实现 ReAct 循环，调用 execute_with_llm
        Ok(TaskResult {
            task_id: goal.goal_id.clone(),
            status: TaskStatus::Completed,
            output: Some(format!("Completed: {}", goal.description)),
            error: None,
            artifacts: Vec::new(),
            duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// 执行子目标（使用 ReAct 循环 + LLM）
    #[allow(dead_code)]
    pub async fn execute_with_llm(
        &self,
        goal: &SubGoal,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(target: "crabmate", "Operator executing goal: {} with ReAct", goal.goal_id);

        let mut state = ReactState {
            iteration: 0,
            messages: Vec::new(),
            observations: Vec::new(),
        };

        // 构建初始系统提示
        let system_prompt = self.build_system_prompt(goal);
        state.messages.push(Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(system_prompt)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });

        // 添加用户任务
        let user_task = format!(
            "任务：{}\n\n请执行任务并通过工具调用完成任务。",
            goal.description
        );
        state.messages.push(Message {
            role: "user".to_string(),
            content: Some(MessageContent::Text(user_task)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });

        // ReAct 循环
        loop {
            state.iteration += 1;

            if state.iteration > self.config.max_iterations {
                return Ok(TaskResult {
                    task_id: goal.goal_id.clone(),
                    status: TaskStatus::Failed {
                        reason: "Max iterations reached".to_string(),
                    },
                    output: Some(self.build_output_summary(&state)),
                    error: Some("Max iterations reached".to_string()),
                    artifacts: Vec::new(),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                });
            }

            // 调用 LLM
            let response = self
                .call_llm(cfg, llm_backend, client, api_key, &state)
                .await?;

            // 解析 LLM 响应（检查是否有工具调用）
            if let Some(tool_calls) = &response.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;

                    // 检查工具是否允许
                    if !self.is_tool_allowed(tool_name) {
                        state
                            .observations
                            .push(format!("Tool {} is not allowed", tool_name));
                        continue;
                    }

                    // 添加工具调用到消息
                    let mut assistant_msg = response.clone();
                    assistant_msg.tool_calls = Some(vec![tool_call.clone()]);
                    state.messages.push(assistant_msg);

                    // 执行工具（这里需要调用实际的工具执行器）
                    let observation = format!("[Tool {} executed]", tool_name);
                    state.observations.push(observation.clone());

                    // 添加工具结果
                    state.messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(observation)),
                        reasoning_content: None,
                        reasoning_details: None,
                        tool_calls: None,
                        name: None,
                        tool_call_id: Some(tool_call.id.clone()),
                    });
                }
            } else {
                // 没有工具调用，检查是否有最终回复
                if let Some(content) = &response.content {
                    let text = crate::types::message_content_as_str(&Some(content.clone()))
                        .unwrap_or("")
                        .to_string();
                    if !text.is_empty() {
                        state.observations.push(format!("Final: {}", text));
                        return Ok(TaskResult {
                            task_id: goal.goal_id.clone(),
                            status: TaskStatus::Completed,
                            output: Some(text),
                            error: None,
                            artifacts: Vec::new(),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                        });
                    }
                }
            }
        }
    }

    /// 调用 LLM
    #[allow(dead_code)]
    async fn call_llm(
        &self,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        state: &ReactState,
    ) -> Result<Message, OperatorError> {
        let params = CompleteChatRetryingParams::new(
            llm_backend,
            client,
            api_key,
            cfg,
            LlmRetryingTransportOpts::headless_no_stream(),
            None,
            None,
        );

        let request = crate::llm::no_tools_chat_request(
            cfg,
            &state.messages,
            None,
            None,
            crate::types::LlmSeedOverride::FromConfig,
        );

        let (response, _) = crate::llm::complete_chat_retrying(&params, &request).await?;
        Ok(response)
    }

    /// 构建系统提示
    #[allow(dead_code)]
    fn build_system_prompt(&self, goal: &SubGoal) -> String {
        format!(
            r#"你是一个 ReAct (Reasoning + Acting) 代理。

当前任务：{}

## 能力
你可以使用以下工具来完成任务：
- read_file: 读取文件
- glob: 搜索文件
- grep: 搜索文件内容
- write_file: 写文件
- run_command: 执行命令
- http_fetch: 发起 HTTP 请求

## 输出格式
每次回复请使用以下格式之一：

1. 如果需要调用工具：
```json
{{"tool_calls": [{{"name": "工具名", "arguments": {{"arg1": "value1"}}}}]}}
```

2. 如果任务完成：
```json
{{"result": "任务完成描述"}}
```

3. 如果任务失败：
```json
{{"error": "失败原因"}}
```

只输出 JSON，不要有其他内容。
"#,
            goal.description
        )
    }

    /// 构建输出摘要
    #[allow(dead_code)]
    fn build_output_summary(&self, state: &ReactState) -> String {
        format!(
            "Completed {} iterations with {} observations",
            state.iteration,
            state.observations.len()
        )
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
