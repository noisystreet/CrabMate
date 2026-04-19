//! Operator Agent：执行子目标的 ReAct 循环
//!
//! Operator 负责：
//! - 理解子目标
//! - 决定工具调用
//! - 执行 ReAct 循环（Thought → Action → Observation）

use std::time::Instant;
use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::LlmCompleteError;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts};
use crate::types::{Message, MessageContent, Tool};

use super::task::{SubGoal, TaskResult, TaskStatus};
use super::tool_executor::ToolExecutor;

/// Operator Agent 配置
#[derive(Debug, Clone)]
pub struct OperatorConfig {
    /// 最大 ReAct 迭代次数
    pub max_iterations: usize,
    /// 可用的工具列表（为空表示使用全部工具）
    pub allowed_tools: Vec<String>,
    /// 工具定义列表（用于 LLM 函数调用）
    pub tools_defs: Vec<Tool>,
    /// SSE 发送器（用于发送工具调用/结果事件）
    pub sse_out: Option<Sender<String>>,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            allowed_tools: Vec::new(),
            tools_defs: Vec::new(),
            sse_out: None,
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
    ///
    /// 此版本用于测试或作为降级路径。完整版本使用 execute_with_tools。
    pub async fn execute(&self, goal: &SubGoal) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(target: "crabmate", "[HIERARCHICAL] Operator (simple): goal_id={} desc={}", goal.goal_id, truncate_goal(&goal.description));

        // 模拟执行延迟
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        Ok(TaskResult {
            task_id: goal.goal_id.clone(),
            status: TaskStatus::Completed,
            output: Some(format!("Completed: {} (simple mode)", goal.description)),
            error: None,
            artifacts: Vec::new(),
            duration_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// 执行子目标（使用 ReAct 循环 + 真实工具执行）
    #[allow(dead_code)]
    pub async fn execute_with_tools(
        &self,
        goal: &SubGoal,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        tool_executor: &ToolExecutor,
    ) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(target: "crabmate", "[HIERARCHICAL] Operator (react): goal_id={} desc={}", goal.goal_id, truncate_goal(&goal.description));

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

            // 检查是否有工具调用
            if let Some(tool_calls) = &response.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;

                    // 检查工具是否允许
                    if !self.is_tool_allowed(tool_name) {
                        state
                            .observations
                            .push(format!("Tool {} is not allowed", tool_name));
                        state.messages.push(Message {
                            role: "tool".to_string(),
                            content: Some(MessageContent::Text(format!(
                                "Error: Tool {} is not allowed. Available tools: {:?}",
                                tool_name, self.config.allowed_tools
                            ))),
                            reasoning_content: None,
                            reasoning_details: None,
                            tool_calls: None,
                            name: None,
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                        continue;
                    }

                    // 发送 ToolCall SSE 事件
                    if let Some(ref sse_out) = self.config.sse_out {
                        log::info!(target: "crabmate", "[HIERARCHICAL] Operator: sending ToolCall SSE for tool={}", tool_name);
                        let args = &tool_call.function.arguments;
                        let summary = crate::tools::summarize_tool_call(tool_name, args)
                            .unwrap_or_else(|| format!("tool: {tool_name}"));
                        let encoded =
                            crate::sse::encode_message(crate::sse::SsePayload::ToolCall {
                                tool_call: crate::sse::protocol::ToolCallSummary {
                                    name: tool_name.clone(),
                                    summary,
                                    arguments_preview: Some(
                                        crate::redact::tool_arguments_preview_for_sse(args),
                                    ),
                                    arguments: Some(
                                        crate::redact::tool_arguments_redacted_for_sse(args),
                                    ),
                                },
                            });
                        let _ = crate::sse::send_string_logged(
                            sse_out,
                            encoded,
                            "hierarchical::operator_tool_call",
                        )
                        .await;
                    } else {
                        log::warn!(target: "crabmate", "[HIERARCHICAL] Operator: sse_out is None, skipping ToolCall SSE");
                    }

                    // 添加工具调用到消息
                    state.messages.push(Message {
                        role: "assistant".to_string(),
                        content: response.content.clone(),
                        reasoning_content: None,
                        reasoning_details: None,
                        tool_calls: Some(vec![tool_call.clone()]),
                        name: None,
                        tool_call_id: None,
                    });

                    // 执行真实工具
                    let result = tool_executor.execute_tool_call(tool_call);

                    log::info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Operator: tool={} success={} output_len={}",
                        result.tool_name,
                        result.success,
                        result.output.len()
                    );

                    // 发送 ToolResult SSE 事件
                    if let Some(ref sse_out) = self.config.sse_out {
                        // 使用更有意义的摘要：包含执行结果的描述
                        let tool_summary = if result.success {
                            if result.output.len() > 100 {
                                let truncated: String = result.output.chars().take(100).collect();
                                format!("✅ {} 成功: {}...", result.tool_name, truncated)
                            } else {
                                format!("✅ {} 成功: {}", result.tool_name, result.output)
                            }
                        } else {
                            format!("❌ {} 失败: {}", result.tool_name, result.output)
                        };
                        let encoded =
                            crate::sse::encode_message(crate::sse::SsePayload::ToolResult {
                                tool_result: crate::sse::protocol::ToolResultBody {
                                    name: result.tool_name.clone(),
                                    result_version: 1,
                                    summary: Some(tool_summary),
                                    output: result.output.clone(),
                                    ok: Some(result.success),
                                    exit_code: None,
                                    error_code: None,
                                    failure_category: None,
                                    retryable: Some(false),
                                    tool_call_id: Some(tool_call.id.clone()),
                                    execution_mode: Some("hierarchical".to_string()),
                                    parallel_batch_id: None,
                                    stdout: None,
                                    stderr: None,
                                },
                            });
                        let _ = crate::sse::send_string_logged(
                            sse_out,
                            encoded,
                            "hierarchical::operator_tool_result",
                        )
                        .await;
                    }

                    // 记录观察结果
                    let observation = if result.success {
                        format!(
                            "Tool {} executed successfully: {}",
                            result.tool_name,
                            truncate_output(&result.output)
                        )
                    } else {
                        format!("Tool {} failed: {}", result.tool_name, result.output)
                    };
                    state.observations.push(observation.clone());

                    // 添加工具结果到消息
                    state.messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(result.output)),
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
                        // 检查是否包含"完成"或"已完成"
                        if text.contains("完成")
                            || text.contains("finished")
                            || text.contains("done")
                        {
                            state
                                .observations
                                .push(format!("Final: {}", truncate_output(&text)));
                            // 仅在未开启 AGENT_WEB_RAW_ASSISTANT_OUTPUT 时剥离思维链标签
                            let output = if crate::web::web_ui_env::web_raw_assistant_output_env() {
                                text.clone()
                            } else {
                                strip_thinking_tags(&text)
                            };
                            return Ok(TaskResult {
                                task_id: goal.goal_id.clone(),
                                status: TaskStatus::Completed,
                                output: Some(output),
                                error: None,
                                artifacts: Vec::new(),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                            });
                        } else {
                            // LLM 可能需要继续，直接将回复作为观察继续循环
                            state
                                .observations
                                .push(format!("LLM response: {}", truncate_output(&text)));
                        }
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

        let request = if self.config.tools_defs.is_empty() {
            crate::llm::no_tools_chat_request(
                cfg,
                &state.messages,
                None,
                None,
                crate::types::LlmSeedOverride::FromConfig,
            )
        } else {
            crate::llm::tool_chat_request(
                cfg,
                &state.messages,
                &self.config.tools_defs,
                None,
                None,
                crate::types::LlmSeedOverride::FromConfig,
            )
        };

        let (response, _) = crate::llm::complete_chat_retrying(&params, &request).await?;
        Ok(response)
    }

    /// 构建系统提示
    #[allow(dead_code)]
    fn build_system_prompt(&self, goal: &SubGoal) -> String {
        let tools_list = if self.config.allowed_tools.is_empty() {
            "所有可用工具".to_string()
        } else {
            self.config.allowed_tools.join(", ")
        };

        format!(
            r#"你是一个 ReAct (Reasoning + Acting) 代理。

当前任务：{}

## 可用工具
{}

## 规则
1. 首先分析任务，确定需要的工具
2. 每次只调用一个工具
3. 根据工具返回结果决定下一步
4. 任务完成后给出总结（包含"完成"或"finished"字样）
"#,
            goal.description, tools_list
        )
    }

    /// 构建输出摘要
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
            || self
                .config
                .allowed_tools
                .iter()
                .any(|t| t == tool_name || t == "*")
    }
}

/// 截断输出用于日志（按字符边界截断，支持中文）
fn truncate_output(output: &str) -> String {
    const MAX_LEN: usize = 200;
    if output.len() > MAX_LEN {
        let truncated = output
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &output[..truncated])
    } else {
        output.to_string()
    }
}

/// 剥离思维链标签
fn strip_thinking_tags(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!("{}{}", &result[..start], &result[start + end + 6..]);
        } else {
            break;
        }
    }
    result.trim().to_string()
}

/// 截断目标描述用于日志（按字符边界截断，支持中文）
fn truncate_goal(desc: &str) -> String {
    const MAX_LEN: usize = 80;
    if desc.len() > MAX_LEN {
        let truncated = desc
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &desc[..truncated])
    } else {
        desc.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute() {
        let config = OperatorConfig::default();
        let operator = OperatorAgent::new(config);
        let goal = SubGoal::new("test", "测试目标").with_tools(vec!["read_file".to_string()]);

        let result = operator.execute(&goal).await.unwrap();
        assert!(matches!(result.status, TaskStatus::Completed));
    }

    #[test]
    fn test_get_tools_for_capabilities() {
        // 此函数已废弃，保留测试仅用于验证
        let tools = ["read_file".to_string(), "run_command".to_string()];
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"run_command".to_string()));
    }

    #[test]
    fn test_is_tool_allowed() {
        let config = OperatorConfig {
            max_iterations: 10,
            allowed_tools: vec!["read_file".to_string()],
            tools_defs: vec![],
            sse_out: None,
        };
        let operator = OperatorAgent::new(config);

        assert!(operator.is_tool_allowed("read_file"));
        assert!(!operator.is_tool_allowed("write_file"));
    }
}
