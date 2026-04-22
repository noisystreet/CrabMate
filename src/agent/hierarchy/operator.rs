//! Operator Agent：执行子目标的 ReAct 循环
//!
//! Operator 负责：
//! - 理解子目标
//! - 决定工具调用
//! - 执行 ReAct 循环（Thought → Action → Observation）
//! - 管理构建状态（BuildState）

use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::llm::LlmCompleteError;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts};
use crate::types::{Message, MessageContent, Tool};

use super::artifact_resolver::ArtifactResolver;
use super::artifact_store::ArtifactStore;
use super::build_state::BuildState;
use super::task::{SubGoal, TaskResult, TaskStatus};
use super::tool_executor::ToolExecutor;

/// 编译错误类型
#[derive(Debug, Clone, PartialEq)]
pub enum CompileErrorType {
    /// OpenMP 并行区域错误
    OpenMPError,
    /// 编译器版本不兼容
    CompilerVersionError,
    /// 缺少依赖库
    MissingDependency,
    /// 配置错误（如错误的 arch 配置）
    ConfigError,
    /// 语法错误
    SyntaxError,
    /// 链接错误
    LinkError,
    /// 工作目录错误
    WorkingDirectoryError,
    /// 其他错误
    Other(String),
}

/// 编译错误信息
#[derive(Debug, Clone)]
pub struct CompileErrorInfo {
    /// 错误类型
    pub error_type: CompileErrorType,
    /// 错误描述
    pub description: String,
    /// 建议的修复方案
    pub suggested_fix: String,
    /// 是否可重试
    pub retryable: bool,
    /// 建议的替代配置（如果有）
    pub alternative_config: Option<String>,
}

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
    /// 产物存储（用于状态共享）
    pub artifact_store: Option<ArtifactStore>,
    /// 构建状态（编译任务使用）
    pub build_state: Option<Arc<Mutex<BuildState>>>,
    /// 是否启用编译错误自动修复
    pub enable_compile_error_recovery: bool,
    /// 编译错误重试次数
    pub compile_error_max_retries: usize,
    /// 已尝试的配置模板（用于避免重复尝试）
    pub attempted_configs: Vec<String>,
    /// 是否启用动态子目标分解
    pub enable_dynamic_decomposition: bool,
    /// 动态分解复杂度阈值（达到此分数触发分解）
    pub dynamic_decomposition_threshold: u8,
}

impl Default for OperatorConfig {
    fn default() -> Self {
        Self {
            max_iterations: 15,
            allowed_tools: Vec::new(),
            tools_defs: Vec::new(),
            sse_out: None,
            artifact_store: None,
            build_state: None,
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            attempted_configs: Vec::new(),
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 30,
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
    /// 任务是否已完成（用于提前终止）
    task_completed: bool,
    /// 完成原因
    completion_reason: Option<String>,
    /// 当前工作目录（用于跟踪目录变化）
    current_working_dir: Option<std::path::PathBuf>,
    /// 连续失败计数
    consecutive_failures: usize,
    /// 上次失败的工具名称（用于检测重复失败）
    last_failed_tool: Option<String>,
    /// 上次失败的错误类型
    last_error_type: Option<CompileErrorType>,
    /// 最近执行的命令历史（用于检测重复命令）
    recent_commands: Vec<String>,
    /// 重复命令计数
    duplicate_command_count: usize,
    /// 已使用的工具集合（用于复杂度评估）
    tools_used: std::collections::HashSet<String>,
    /// 动态分解已触发次数
    dynamic_decomposition_count: usize,
}

/// 工具执行结果分析
#[derive(Debug, Clone)]
enum ToolExecutionOutcome {
    /// 普通执行
    Normal,
    /// 任务已完成
    TaskCompleted { reason: String },
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
    #[allow(dead_code, clippy::too_many_arguments)]
    pub async fn execute_with_tools(
        &self,
        goal: &SubGoal,
        cfg: &AgentConfig,
        llm_backend: &dyn ChatCompletionsBackend,
        client: &reqwest::Client,
        api_key: &str,
        tool_executor: &ToolExecutor,
        extra_context: Option<&str>,
    ) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(target: "crabmate", "[HIERARCHICAL] Operator (react): goal_id={} desc={}", goal.goal_id, truncate_goal(&goal.description));

        // 构建产物解析器
        let artifact_store = self.config.artifact_store.as_ref();
        let resolver = artifact_store.map(|store| {
            // 注意：这里我们暂时不传递 build_state 给 resolver，因为生命周期问题
            // 产物路径注入主要依赖 artifact_store
            ArtifactResolver::new(store, None)
        });

        // 如果有构建需求，注入产物信息到上下文
        let enhanced_context = if let Some(ref resolver) = resolver {
            self.build_context_with_artifacts(goal, extra_context, resolver)
        } else {
            extra_context.map(|s| s.to_string())
        };

        let mut state = ReactState {
            iteration: 0,
            messages: Vec::new(),
            observations: Vec::new(),
            task_completed: false,
            completion_reason: None,
            current_working_dir: None,
            consecutive_failures: 0,
            last_failed_tool: None,
            last_error_type: None,
            recent_commands: Vec::new(),
            duplicate_command_count: 0,
            tools_used: std::collections::HashSet::new(),
            dynamic_decomposition_count: 0,
        };

        // 构建初始系统提示（传入当前工作目录）
        let system_prompt = self.build_system_prompt(goal, state.current_working_dir.as_deref());
        state.messages.push(Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(system_prompt)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });

        // 添加用户任务（使用增强后的描述）
        let task_description = if let Some(ctx) = enhanced_context {
            format!("{}\n\n{}", goal.description, ctx)
        } else {
            goal.description.clone()
        };
        let user_task = format!(
            "任务：{}\n\n请执行任务并通过工具调用完成任务。",
            task_description
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
                // 首先添加一个包含所有 tool_calls 的 assistant 消息
                // 这是 OpenAI API 的要求：所有 tool_calls 必须在同一个 assistant 消息中
                state.messages.push(Message {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                    reasoning_content: None,
                    reasoning_details: None,
                    tool_calls: Some(tool_calls.clone()),
                    name: None,
                    tool_call_id: None,
                });

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

                    // 注入产物路径到工具参数
                    let injected_tool_call = if let Some(ref resolver) = resolver {
                        self.inject_artifact_paths_into_tool_call(tool_call, resolver)
                    } else {
                        tool_call.clone()
                    };

                    // 执行真实工具（使用注入后的参数）
                    let result = tool_executor.execute_tool_call(&injected_tool_call).await;

                    log::info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Operator: tool={} success={} output_len={}",
                        result.tool_name,
                        result.success,
                        result.output.len()
                    );

                    // 检测工作目录变化（从工具参数或输出中提取）
                    if let Some(new_dir) =
                        self.detect_working_dir_change(&injected_tool_call, &result)
                    {
                        state.current_working_dir = Some(new_dir);
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: working directory changed to {:?}",
                            state.current_working_dir
                        );
                    }

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

                    // 检测重复命令
                    let command_signature =
                        format!("{}:{}", result.tool_name, tool_call.function.arguments);
                    if state.recent_commands.contains(&command_signature) {
                        state.duplicate_command_count += 1;
                        log::warn!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: detected duplicate command (count={}): {}",
                            state.duplicate_command_count,
                            result.tool_name
                        );

                        // 如果重复执行同一命令超过 2 次，提前终止
                        if state.duplicate_command_count >= 2 {
                            log::warn!(
                                target: "crabmate",
                                "[HIERARCHICAL] Operator: too many duplicate commands ({}), terminating early",
                                state.duplicate_command_count
                            );

                            return Ok(TaskResult {
                                task_id: goal.goal_id.clone(),
                                status: TaskStatus::Failed {
                                    reason: format!(
                                        "检测到重复执行同一命令 {} 次，可能陷入循环。请检查任务逻辑。",
                                        state.duplicate_command_count + 1
                                    ),
                                },
                                output: Some(self.build_output_summary(&state)),
                                error: Some(format!(
                                    "重复命令检测：命令 '{}' 被执行了多次",
                                    result.tool_name
                                )),
                                artifacts: Vec::new(),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    } else {
                        // 新命令，重置重复计数
                        state.duplicate_command_count = 0;
                    }

                    // 保持最近 5 条命令历史
                    state.recent_commands.push(command_signature);
                    if state.recent_commands.len() > 5 {
                        state.recent_commands.remove(0);
                    }

                    // 分析工具执行结果，检查是否表示任务已完成
                    let execution_outcome = self.analyze_tool_execution(&result, goal);

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

                    // 如果工具执行失败且是编译相关命令，分析错误并提供恢复提示
                    let mut error_recovery_hint = None;
                    let mut current_error_type: Option<CompileErrorType> = None;
                    if !result.success
                        && self.config.enable_compile_error_recovery
                        && is_compile_command(&result.tool_name, &tool_call.function.arguments)
                        && let Some(error_info) = self.analyze_compile_error(&result.output)
                        && error_info.retryable
                    {
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: detected retryable compile error: {:?}",
                            error_info.error_type
                        );
                        error_recovery_hint =
                            Some(self.build_compile_error_recovery_hint(&error_info));
                        current_error_type = Some(error_info.error_type.clone());
                    }

                    // 更新失败计数和状态
                    if !result.success {
                        state.consecutive_failures += 1;
                        state.last_failed_tool = Some(result.tool_name.clone());
                        state.last_error_type = current_error_type.clone();

                        // 检查是否连续多次失败同一类型的错误
                        if state.consecutive_failures >= 3 {
                            let failure_reason = if let Some(ref err_type) = current_error_type {
                                format!(
                                    "连续 {} 次执行失败，错误类型: {:?}。请检查工作目录和命令参数是否正确。",
                                    state.consecutive_failures, err_type
                                )
                            } else {
                                format!(
                                    "连续 {} 次执行失败。请检查工作目录和命令参数是否正确。",
                                    state.consecutive_failures
                                )
                            };

                            log::warn!(
                                target: "crabmate",
                                "[HIERARCHICAL] Operator: too many consecutive failures ({}), terminating early",
                                state.consecutive_failures
                            );

                            return Ok(TaskResult {
                                task_id: goal.goal_id.clone(),
                                status: TaskStatus::Failed {
                                    reason: failure_reason.clone(),
                                },
                                output: Some(failure_reason.clone()),
                                error: Some(failure_reason),
                                artifacts: Vec::new(),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    } else {
                        // 成功执行，重置失败计数
                        state.consecutive_failures = 0;
                        state.last_failed_tool = None;
                        state.last_error_type = None;
                    }

                    // 添加工具结果到消息
                    let tool_result_content = if let Some(ref hint) = error_recovery_hint {
                        format!("{}\n\n{}", result.output, hint)
                    } else {
                        result.output.clone()
                    };
                    state.messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(tool_result_content)),
                        reasoning_content: None,
                        reasoning_details: None,
                        tool_calls: None,
                        name: None,
                        tool_call_id: Some(tool_call.id.clone()),
                    });

                    // 如果任务已完成，标记状态并准备返回
                    if let ToolExecutionOutcome::TaskCompleted { reason } = execution_outcome {
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: task completion detected after tool={}: {}",
                            result.tool_name,
                            reason
                        );
                        state.task_completed = true;
                        state.completion_reason = Some(reason);
                    }

                    // 从工具结果中提取产物并更新 BuildState
                    if let Some(ref build_state_arc) = self.config.build_state
                        && let Ok(mut build_state) = build_state_arc.lock()
                    {
                        for artifact in &result.extracted_artifacts {
                            log::info!(
                                target: "crabmate",
                                "[HIERARCHICAL] Operator: recording artifact {:?} from tool={}",
                                artifact.path,
                                artifact.source_tool
                            );

                            // 根据产物类型更新 BuildState
                            match artifact.kind {
                                super::tool_executor::ExtractedArtifactKind::SourceFile => {
                                    // 尝试读取源文件内容并记录
                                    if let Ok(content) = std::fs::read_to_string(&artifact.path) {
                                        build_state.record_source_file(&artifact.path, &content);
                                    }
                                }
                                super::tool_executor::ExtractedArtifactKind::ObjectFile => {
                                    build_state.add_object_file(artifact.path.clone());
                                }
                                super::tool_executor::ExtractedArtifactKind::Executable => {
                                    build_state.add_executable(artifact.path.clone());
                                }
                                super::tool_executor::ExtractedArtifactKind::StaticLibrary => {
                                    build_state.add_static_library(artifact.path.clone());
                                }
                                super::tool_executor::ExtractedArtifactKind::DynamicLibrary => {
                                    build_state.add_dynamic_library(artifact.path.clone());
                                }
                                super::tool_executor::ExtractedArtifactKind::BuildDirectory => {
                                    build_state.set_build_dir(artifact.path.clone());
                                }
                                _ => {}
                            }
                        }
                    }

                    // 如果任务已标记完成，提前终止 ReAct 循环
                    if state.task_completed {
                        let reason = state.completion_reason.clone().unwrap_or_default();
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: terminating ReAct loop early, task completed: {}",
                            reason
                        );
                        return Ok(TaskResult {
                            task_id: goal.goal_id.clone(),
                            status: TaskStatus::Completed,
                            output: Some(format!("Task completed: {}", reason)),
                            error: None,
                            artifacts: Vec::new(),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                        });
                    }

                    // 记录使用的工具
                    state.tools_used.insert(result.tool_name.clone());

                    // 动态子目标分解检查
                    if self.config.enable_dynamic_decomposition
                        && state.dynamic_decomposition_count == 0 // 只触发一次
                        && state.iteration >= 5
                    // 至少执行了5轮
                    {
                        let decomposer = super::dynamic_decomposer::DynamicDecomposer::new();
                        let assessment = decomposer.assess_complexity(
                            goal,
                            state.iteration,
                            state.consecutive_failures,
                            state.tools_used.len(),
                        );

                        if assessment.needs_decomposition
                            && assessment.score >= self.config.dynamic_decomposition_threshold
                        {
                            log::info!(
                                target: "crabmate",
                                "[HIERARCHICAL] Operator: complexity assessment triggered decomposition (score={})",
                                assessment.score
                            );

                            // 返回特殊结果，表示需要动态分解
                            let reason = assessment.reason.clone();
                            return Ok(TaskResult {
                                task_id: goal.goal_id.clone(),
                                status: TaskStatus::NeedsDecomposition {
                                    reason: assessment.reason,
                                    suggested_subgoals: assessment.suggested_subgoals,
                                },
                                output: Some(format!(
                                    "任务过于复杂（复杂度评分: {}），建议分解为 {} 个子目标。原因: {}",
                                    assessment.score, assessment.suggested_subgoals, reason
                                )),
                                error: None,
                                artifacts: Vec::new(),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                            });
                        }
                    }
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
                            // LLM 可能需要继续，将回复作为观察并添加到消息历史
                            state
                                .observations
                                .push(format!("LLM response: {}", truncate_output(&text)));
                            // 重要：将 LLM 回复添加到 messages，否则上下文会丢失
                            state.messages.push(Message {
                                role: "assistant".to_string(),
                                content: Some(MessageContent::Text(text.clone())),
                                reasoning_content: None,
                                reasoning_details: None,
                                tool_calls: None,
                                name: None,
                                tool_call_id: None,
                            });
                        }
                    } else {
                        // LLM 返回空内容，添加一个提示继续
                        log::warn!(target: "crabmate", "[HIERARCHICAL] Operator: LLM returned empty content, adding prompt to continue");
                        state.messages.push(Message {
                            role: "user".to_string(),
                            content: Some(MessageContent::Text("请继续执行任务。".to_string())),
                            reasoning_content: None,
                            reasoning_details: None,
                            tool_calls: None,
                            name: None,
                            tool_call_id: None,
                        });
                    }
                } else {
                    // LLM 没有返回任何内容，添加一个提示继续
                    log::warn!(target: "crabmate", "[HIERARCHICAL] Operator: LLM returned no content, adding prompt to continue");
                    state.messages.push(Message {
                        role: "user".to_string(),
                        content: Some(MessageContent::Text("请继续执行任务。".to_string())),
                        reasoning_content: None,
                        reasoning_details: None,
                        tool_calls: None,
                        name: None,
                        tool_call_id: None,
                    });
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

    /// 分析工具执行结果，判断是否表示任务已完成
    fn analyze_tool_execution(
        &self,
        result: &super::tool_executor::ToolExecutionResult,
        goal: &SubGoal,
    ) -> ToolExecutionOutcome {
        if !result.success {
            return ToolExecutionOutcome::Normal;
        }

        let output = &result.output;
        let tool_name = &result.tool_name;

        // 1. 检查是否成功运行了可执行文件并产生预期输出
        if tool_name == "run_command" || tool_name.starts_with("./") {
            // 检查是否运行了可执行文件并输出 Hello World
            if output.contains("Hello") || output.contains("hello") || output.contains("world") {
                return ToolExecutionOutcome::TaskCompleted {
                    reason: "Program executed successfully with expected output".to_string(),
                };
            }
            // 检查是否是 ELF 文件的 file 命令输出（验证步骤）
            if output.contains("ELF") && output.contains("executable") {
                // 这是验证步骤，不是真正的任务完成
                return ToolExecutionOutcome::Normal;
            }
        }

        // 2. 检查是否成功编译并链接了可执行文件
        if tool_name == "run_command" || tool_name == "cmake" || tool_name == "make" {
            // 匹配 cmake 构建成功输出
            if output.contains("[100%]")
                && output.contains("Linking")
                && output.contains("executable")
            {
                // 提取可执行文件名
                if let Some(line) = output
                    .lines()
                    .find(|l| l.contains("Linking") && l.contains("executable"))
                    && let Some(name) = line.split_whitespace().last()
                {
                    return ToolExecutionOutcome::TaskCompleted {
                        reason: format!("Build completed: executable '{}' generated", name),
                    };
                }
            }
        }

        // 3. 根据目标描述判断
        let goal_desc = goal.description.to_lowercase();

        // 如果目标是运行程序并看到输出
        if goal_desc.contains("运行")
            || goal_desc.contains("执行")
            || goal_desc.contains("run")
            || goal_desc.contains("execute")
        {
            // 检查输出中是否有程序运行的典型特征
            if output.contains("Hello")
                || output.contains("World")
                || output.contains("hello")
                || output.contains("world")
            {
                return ToolExecutionOutcome::TaskCompleted {
                    reason: "Program executed and produced output".to_string(),
                };
            }
        }

        // 如果目标是编译
        if goal_desc.contains("编译") || goal_desc.contains("build") || goal_desc.contains("make")
        {
            // 检查是否成功生成了构建产物
            if !result.extracted_artifacts.is_empty() {
                for artifact in &result.extracted_artifacts {
                    if matches!(
                        artifact.kind,
                        super::tool_executor::ExtractedArtifactKind::Executable
                    ) {
                        return ToolExecutionOutcome::TaskCompleted {
                            reason: format!(
                                "Build completed: {} generated",
                                artifact.path.display()
                            ),
                        };
                    }
                }
            }
        }

        ToolExecutionOutcome::Normal
    }

    /// 构建系统提示
    fn build_system_prompt(&self, goal: &SubGoal, current_dir: Option<&std::path::Path>) -> String {
        let tools_list = if self.config.allowed_tools.is_empty() {
            "所有可用工具".to_string()
        } else {
            self.config.allowed_tools.join(", ")
        };

        // 根据目标描述添加特定的执行指导
        let execution_guide = self.build_execution_guide(goal);

        // 添加当前工作目录信息
        let working_dir_info = current_dir
            .map(|d| format!("\n## 当前工作目录\n{}\n", d.display()))
            .unwrap_or_default();

        format!(
            r#"你是一个 ReAct (Reasoning + Acting) 代理。

当前任务：{}{}

## 可用工具
{}

## 执行指导
{}

## 规则
1. 首先分析任务，确定需要的工具
2. 每次只调用一个工具
3. 根据工具返回结果决定下一步
4. 任务完成后给出总结（包含"完成"或"finished"字样）

## 重要约束
- **禁止假设**任何文件或目录存在。调用 `read_dir`、`search_replace`、`modify_file` 等工具前，**必须先用 `read_dir` 确认目标路径存在**
- 如果工具返回"路径无法解析"或"No such file or directory"，**必须承认路径不存在**，不能再用相同的错误路径继续操作
- 如果不确定某个路径是否存在，先用 `read_dir` 的父目录来确认
- **创建文件必须使用 `create_file` 工具**，禁止使用 `echo`、`cat`、`tee` 等命令通过 `run_command` 创建文件
- `create_file` 的 `content` 参数：在 JSON 中必须使用正确的转义序列，换行用 `\n`，制表用 `\t`，双引号用 `\"

## 工作目录管理（关键！）
- **当前工作目录**已在上方"当前工作目录"部分显示
- **所有相对路径都是基于当前工作目录的**
- 如果需要在子目录中执行命令，有三种方式（按推荐顺序）：
  1. **使用 `-C` 选项**（推荐）：`make -C subdirectory`
  2. **使用完整路径作为 command**：`command: "subdirectory/script.sh"`, `args: []`
  3. **使用完整路径在 args 中**：`command: "cp"`, `args: ["subdirectory/src", "subdirectory/dest"]`
- **禁止**使用 `cd` 命令后再执行其他命令（cd 不会持久化工作目录）
- **常见错误示例**：
  - ❌ 错误：`command: "./configure"`, `args: ["subdirectory/configure"]` —— command 和 args 重复了路径
  - ❌ 错误：`command: "cp"`, `args: ["setup/file", "dest"]` —— 当前目录下没有 setup/ 目录
  - ✅ 正确：`command: "cp"`, `args: ["subdirectory/setup/file", "subdirectory/dest"]` —— 使用完整路径
  - ✅ 正确：`command: "subdirectory/configure"`, `args: []` —— 完整路径作为 command
- 如果命令返回"找不到文件"或"No such file or directory"，首先检查工作目录是否正确
"#,
            goal.description, working_dir_info, tools_list, execution_guide
        )
    }

    /// 根据目标类型构建执行指导
    fn build_execution_guide(&self, goal: &SubGoal) -> String {
        let desc = goal.description.to_lowercase();

        // 编译/构建类任务
        if desc.contains("编译")
            || desc.contains("构建")
            || desc.contains("build")
            || desc.contains("make")
            || desc.contains("cmake")
        {
            return r#"这是一个编译/构建任务，请按以下步骤执行：

**步骤 0: 确定工作目录（关键！）**
- 如果源码在子目录中（如 `hpcg-HPCG-release-3-1-0/`），**所有后续命令必须在正确的目录中执行**
- **当前工作目录**是固定的，不会因为你执行了 `cd` 而改变
- **执行命令的三种方式**（按推荐顺序）：
  1. **使用 `-C` 选项**（最推荐）：`make -C hpcg-HPCG-release-3-1-0`
  2. **完整路径作为 command**：`command: "hpcg-HPCG-release-3-1-0/configure"`, `args: []`
  3. **完整路径在 args 中**：`command: "cp"`, `args: ["hpcg-HPCG-release-3-1-0/setup/file", "hpcg-HPCG-release-3-1-0/dest"]`
- **常见错误**：
  - ❌ `command: "./configure"`, `args: ["hpcg-HPCG-release-3-1-0/configure"]` —— 路径重复
  - ❌ `command: "cp"`, `args: ["setup/Make.Linux_Serial", "Make.custom"]` —— 当前目录下没有 setup/
  - ✅ `command: "cp"`, `args: ["hpcg-HPCG-release-3-1-0/setup/Make.Linux_Serial", "hpcg-HPCG-release-3-1-0/Make.custom"]`
- **重要**：复制配置文件、执行 make 等操作都要使用完整路径或在正确的目录中使用 `-C` 选项

**步骤 1: 检测构建系统**
- 使用 `read_dir` 查看源码目录结构（注意使用正确的路径）
- 检查是否存在以下构建文件（按优先级）：
  * CMakeLists.txt → 使用 cmake 构建
  * configure 脚本 → 使用 ./configure && make
  * Makefile → 使用 make
  * build.gradle/pom.xml → Java 项目
  * package.json → Node.js 项目

**步骤 2: 检查编译器/工具链**
- 使用 `which` 检查必要的编译器是否存在（gcc/g++, cmake, make 等）
- 如果编译器不存在，报告错误并终止（不要反复尝试不同的 which 组合）

**步骤 3: 执行构建（注意工作目录）**
- CMake 项目：
  1. `mkdir -p build && cd build`
  2. `cmake ..` 或 `cmake -S .. -B .`
  3. `cmake --build .` 或 `make`
- Configure 项目（如 HPCG）：
  1. 先尝试 `./configure`（在源码目录）
  2. 如果 `./configure` 不在白名单，按以下顺序尝试：
     a. 检查是否有 `setup/` 目录（在源码目录中）
     b. 使用 `read_file` 查看 `setup/` 中的配置模板（如 `Make.GCC_OMP`、`Make.Linux_Serial`）
     c. **在源码目录中**执行 `cp setup/Make.Linux_Serial Make.custom`（选择适合当前系统的模板）
     d. **在源码目录中**执行 `make` 或 `make arch=Linux_Serial` 进行编译
  3. 如果上述都失败，报告错误
- Make 项目：
  1. **在源码目录中**执行 `make`

**步骤 4: 处理编译错误（如果步骤 3 失败）**
如果编译失败，请分析错误类型并采取相应措施：

- **OpenMP 错误**（如 "'n' not specified in enclosing 'parallel'"）：
  * 原因：当前编译器版本与 OpenMP 配置不兼容
  * 解决：切换到非 OpenMP 配置，如 `cp setup/Make.Linux_Serial Make.custom && make`

- **缺少依赖库**（如 "cannot find -lxxx"）：
  * 原因：系统缺少必要的开发库
  * 解决：尝试安装依赖或切换到不需要该库的配置

- **编译器版本不兼容**（如 "unrecognized command line option"）：
  * 原因：配置模板使用了当前编译器不支持的选项
  * 解决：切换到更基础的配置模板

- **配置错误**（如 "Please specify 'arch' variable"）：
  * 原因：Makefile 需要指定 arch 参数
  * 解决：使用 `make arch=Linux_Serial` 或复制配置到 Make.custom

- **工作目录错误**（如 "没有指明目标并且找不到 makefile"）：
  * 原因：当前目录不是源码目录
  * 解决：使用 `-C` 选项指定正确的源码目录，如 `make -C hpcg-HPCG-release-3-1-0`

**步骤 5: 验证构建结果**
- 使用 `read_dir` 或 `run_command ls` 检查是否生成了可执行文件
- 如果构建成功，报告生成的可执行文件路径

**重要约束**：
- 如果步骤 2 发现编译器不存在，直接报告失败，不要继续尝试构建
- 如果命令返回"不在白名单中"，**不要**用其他 shell（bash/sh）重复尝试同一命令，这不会成功
- 最多尝试 3 种不同的构建方式，如果都失败则报告错误
- 对于 configure 项目，优先使用 setup/ 目录中的配置模板，而不是反复尝试 ./configure
- **编译失败后必须尝试其他配置**，不要重复尝试相同的配置"#
                .to_string();
        }

        // 检查编译工具类任务
        if desc.contains("检查")
            && (desc.contains("编译")
                || desc.contains("工具")
                || desc.contains("gcc")
                || desc.contains("make")
                || desc.contains("mpi"))
        {
            return r#"这是一个检查编译工具的任务：

**目标**：确认系统已安装必要的编译工具（gcc、g++、make、cmake、mpi 等）

**执行步骤**：
1. 使用 `which` 命令检查核心工具是否存在：
   - `which gcc g++ make`（基础编译工具）
   - `which cmake`（CMake 构建工具）
   - `which mpicc mpicxx mpirun`（MPI 并行工具，如需要）
   - `which gfortran`（Fortran 编译器，如需要）

2. 如果上述工具都存在，**立即报告任务完成**，无需重复检查其他工具

3. 如果某个工具不存在，报告缺失的工具名称

**重要约束**：
- **不要重复执行相同的 which 命令** - 一旦检测到工具存在，就不要再次检查
- **检测完成即结束** - 确认所有必需工具存在后，立即输出"工具检查完成"并结束任务
- 最多执行 3-5 个 which 命令，不要陷入无限检查循环"#
                .to_string();
        }

        // 文件操作类任务
        if desc.contains("创建")
            || desc.contains("修改")
            || desc.contains("编辑")
            || desc.contains("写入")
        {
            return r#"这是一个文件操作任务：

**步骤 1: 确认路径**
- 使用 `read_dir` 确认目标目录存在
- 如果要修改文件，先用 `read_file` 查看当前内容

**步骤 2: 执行操作**
- 创建文件：使用 `create_file` 工具
- 修改文件：使用 `search_replace` 工具
- 删除文件：使用 `delete_file` 工具

**步骤 3: 验证**
- 使用 `read_file` 确认操作结果

**重要**：禁止假设文件存在，必须先确认再操作。"#
                .to_string();
        }

        // 默认指导
        "分析任务需求，选择合适的工具，逐步执行并验证结果。".to_string()
    }

    /// 分析编译错误并返回错误信息
    fn analyze_compile_error(&self, error_output: &str) -> Option<CompileErrorInfo> {
        let _error_lower = error_output.to_lowercase();

        // OpenMP 错误
        if error_output.contains("not specified in enclosing 'parallel'")
            || error_output.contains("not specified in enclosing parallel")
            || error_output.contains("#pragma omp")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::OpenMPError,
                description: "OpenMP 并行区域变量声明错误".to_string(),
                suggested_fix: "切换到非 OpenMP 配置模板，如 Make.Linux_Serial".to_string(),
                retryable: true,
                alternative_config: Some("Make.Linux_Serial".to_string()),
            });
        }

        // 缺少依赖库
        if error_output.contains("cannot find -l")
            || error_output.contains("cannot find library")
            || error_output.contains("No such file or directory")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::MissingDependency,
                description: "缺少必要的依赖库".to_string(),
                suggested_fix: "尝试安装缺失的库或切换到不需要该库的配置".to_string(),
                retryable: true,
                alternative_config: None,
            });
        }

        // 编译器版本不兼容
        if error_output.contains("unrecognized command line option")
            || error_output.contains("unknown option")
            || error_output.contains("invalid option")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::CompilerVersionError,
                description: "编译器版本不兼容，配置使用了不支持的选项".to_string(),
                suggested_fix: "切换到更基础的配置模板（如 Make.Linux_Serial）".to_string(),
                retryable: true,
                alternative_config: Some("Make.Linux_Serial".to_string()),
            });
        }

        // 配置错误（需要指定 arch）
        if error_output.contains("Please specify 'arch' variable")
            || error_output.contains("arch variable")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::ConfigError,
                description: "Makefile 需要指定 arch 参数".to_string(),
                suggested_fix: "使用 make arch=Linux_Serial 或复制配置到 Make.custom".to_string(),
                retryable: true,
                alternative_config: Some("Make.Linux_Serial".to_string()),
            });
        }

        // 工作目录错误
        if error_output.contains("没有指明目标并且找不到 makefile")
            || error_output.contains("No targets specified and no makefile found")
            || error_output.contains("没有那个文件或目录")
            || error_output.contains("No such file or directory")
            || error_output.contains("cannot find")
            || error_output.contains("无法找到")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::WorkingDirectoryError,
                description: "工作目录错误或找不到构建文件".to_string(),
                suggested_fix: "使用 -C 选项指定正确的源码目录，如 make -C hpcg-HPCG-release-3-1-0"
                    .to_string(),
                retryable: true,
                alternative_config: None,
            });
        }

        // 链接错误
        if error_output.contains("undefined reference")
            || error_output.contains("linker error")
            || error_output.contains("ld: ")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::LinkError,
                description: "链接错误".to_string(),
                suggested_fix: "检查依赖库是否正确链接".to_string(),
                retryable: false,
                alternative_config: None,
            });
        }

        // 语法错误
        if error_output.contains("error: expected")
            || error_output.contains("error: syntax")
            || error_output.contains("error: invalid")
        {
            return Some(CompileErrorInfo {
                error_type: CompileErrorType::SyntaxError,
                description: "源代码语法错误".to_string(),
                suggested_fix: "检查源代码语法".to_string(),
                retryable: false,
                alternative_config: None,
            });
        }

        None
    }

    /// 构建编译错误恢复提示
    fn build_compile_error_recovery_hint(&self, error_info: &CompileErrorInfo) -> String {
        format!(
            r#"检测到编译错误：{}

错误类型：{:?}
建议修复方案：{}
{}

请在下一步工具调用中应用上述修复方案。"#,
            error_info.description,
            error_info.error_type,
            error_info.suggested_fix,
            if let Some(ref config) = error_info.alternative_config {
                format!("\n建议尝试的配置模板：{}", config)
            } else {
                String::new()
            }
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

    /// 构建包含产物信息的上下文
    fn build_context_with_artifacts(
        &self,
        goal: &SubGoal,
        extra_context: Option<&str>,
        resolver: &ArtifactResolver<'_>,
    ) -> Option<String> {
        let mut parts = Vec::new();

        // 添加原始上下文
        if let Some(ctx) = extra_context {
            parts.push(ctx.to_string());
        }

        // 如果有构建需求，添加可用产物信息
        if !goal.build_requirements.needs_artifacts.is_empty() {
            let resolved =
                resolver.resolve_build_requirements(&goal.build_requirements.needs_artifacts);
            let mut artifact_info = vec!["可用构建产物:".to_string()];

            for (kind, path) in resolved {
                let kind_name = format!("{:?}", kind);
                match path {
                    Some(p) => artifact_info.push(format!("  - {}: {}", kind_name, p.display())),
                    None => artifact_info.push(format!("  - {}: (未找到)", kind_name)),
                }
            }

            if artifact_info.len() > 1 {
                parts.push(artifact_info.join("\n"));
            }
        }

        // 添加所有可用产物的摘要
        let artifact_summary = resolver.format_for_llm();
        if artifact_summary != "无可用产物" {
            parts.push(artifact_summary);
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
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

    /// 将产物路径注入到工具调用参数中
    ///
    /// 解析工具参数中的占位符（如 `{artifact:main.cpp}`），
    /// 并将其替换为实际产物路径
    fn inject_artifact_paths_into_tool_call(
        &self,
        tool_call: &crate::types::ToolCall,
        resolver: &ArtifactResolver<'_>,
    ) -> crate::types::ToolCall {
        let mut modified_call = tool_call.clone();

        // 解析参数 JSON
        if let Ok(mut args) =
            serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
        {
            let modified = Self::inject_paths_into_value(&mut args, resolver);

            if modified {
                // 重新序列化参数
                if let Ok(new_args) = serde_json::to_string(&args) {
                    modified_call.function.arguments = new_args;
                    log::info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Operator: injected artifact paths into tool={}",
                        tool_call.function.name
                    );
                }
            }
        }

        modified_call
    }

    /// 递归地将产物路径注入到 JSON 值中
    fn inject_paths_into_value(
        value: &mut serde_json::Value,
        resolver: &ArtifactResolver<'_>,
    ) -> bool {
        let mut modified = false;

        match value {
            serde_json::Value::String(s) => {
                // 检查字符串是否包含占位符
                let mut result = s.clone();

                // 查找所有 {artifact:name} 模式
                let pattern = "{artifact:";
                let mut start = 0;
                while let Some(idx) = result[start..].find(pattern) {
                    let actual_idx = start + idx;
                    if let Some(end_idx) = result[actual_idx..].find('}') {
                        let end = actual_idx + end_idx;
                        let artifact_name = &result[actual_idx + pattern.len()..end];

                        // 尝试解析产物路径
                        if let Some(path) = resolver
                            .resolve_source_file(artifact_name)
                            .or_else(|| resolver.resolve_build_artifact(artifact_name))
                        {
                            let path_str = path.to_string_lossy().to_string();
                            result.replace_range(actual_idx..=end, &path_str);
                            modified = true;
                            // 更新 start 位置，因为字符串长度可能改变
                            start = actual_idx + path_str.len();
                        } else {
                            // 未找到产物，跳过这个占位符
                            start = end + 1;
                        }
                    } else {
                        break;
                    }
                }

                if modified {
                    *s = result;
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr.iter_mut() {
                    if Self::inject_paths_into_value(item, resolver) {
                        modified = true;
                    }
                }
            }
            serde_json::Value::Object(map) => {
                for (_, v) in map.iter_mut() {
                    if Self::inject_paths_into_value(v, resolver) {
                        modified = true;
                    }
                }
            }
            _ => {}
        }

        modified
    }

    /// 检测工作目录变化
    ///
    /// 从工具调用参数和输出中提取工作目录变化信息
    fn detect_working_dir_change(
        &self,
        tool_call: &crate::types::ToolCall,
        result: &super::tool_executor::ToolExecutionResult,
    ) -> Option<std::path::PathBuf> {
        let tool_name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        // 处理 run_command 工具
        if tool_name == "run_command" {
            // 尝试解析参数
            if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args) {
                // 检查是否有明确的 cwd 参数
                if let Some(cwd) = args_json.get("cwd").and_then(|v| v.as_str()) {
                    return Some(std::path::PathBuf::from(cwd));
                }

                // 检查命令参数中是否包含目录切换
                if let Some(command) = args_json.get("command").and_then(|v| v.as_str())
                    && command == "cd"
                {
                    // 检测 cd 命令
                    if let Some(dir) = args_json
                        .get("args")
                        .and_then(|a| a.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.as_str())
                    {
                        return Some(std::path::PathBuf::from(dir));
                    }
                }
            }

            // 从输出中检测 make 的工作目录
            // 例如："make: 进入目录"/home/gzz/test/hpcg-HPCG-release-3-1-0""
            if (result.output.contains("make: 进入目录")
                || result.output.contains("make: Entering directory"))
                && let Some(start) = result
                    .output
                    .find("make: 进入目录\"")
                    .or_else(|| result.output.find("make: Entering directory `"))
            {
                let search_start = start + "make: 进入目录\"".len();
                if let Some(end) = result.output[search_start..]
                    .find('"')
                    .or_else(|| result.output[search_start..].find('\''))
                {
                    let dir = &result.output[search_start..search_start + end];
                    return Some(std::path::PathBuf::from(dir));
                }
            }
        }

        // 处理 cmake 工具
        if tool_name == "cmake"
            && let Ok(args_json) = serde_json::from_str::<serde_json::Value>(args)
        {
            // 检查 -B 参数（构建目录）
            if let Some(build_dir) = args_json.get("build_dir").and_then(|v| v.as_str()) {
                return Some(std::path::PathBuf::from(build_dir));
            }
        }

        None
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
            let close_tag = "</think>";
            result = format!(
                "{}{}",
                &result[..start],
                &result[start + end + close_tag.len()..]
            );
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

/// 判断工具调用是否是编译相关命令
fn is_compile_command(tool_name: &str, args: &str) -> bool {
    // 检查是否是 run_command 工具且参数包含编译相关命令
    if tool_name != "run_command" {
        return false;
    }

    let args_lower = args.to_lowercase();
    let compile_keywords = [
        "make",
        "cmake",
        "gcc",
        "g++",
        "clang",
        "clang++",
        "configure",
        "build",
        "compile",
        "arch=",
    ];

    compile_keywords.iter().any(|kw| args_lower.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::hierarchy::artifact_store::ArtifactStore;
    use crate::agent::hierarchy::task::{Artifact, ArtifactKind};

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
            artifact_store: None,
            build_state: None,
            enable_compile_error_recovery: true,
            compile_error_max_retries: 3,
            attempted_configs: Vec::new(),
            enable_dynamic_decomposition: true,
            dynamic_decomposition_threshold: 30,
        };
        let operator = OperatorAgent::new(config);

        assert!(operator.is_tool_allowed("read_file"));
        assert!(!operator.is_tool_allowed("write_file"));
    }

    #[test]
    fn test_inject_artifact_paths_into_tool_call() {
        // 创建测试产物存储
        let mut store = ArtifactStore::new();
        store.put(
            Artifact::new(
                "1",
                "main.cpp",
                ArtifactKind::BuildArtifact(
                    crate::agent::hierarchy::task::BuildArtifactKind::SourceFile,
                ),
                "goal_1",
            )
            .with_path("/workspace/src/main.cpp"),
        );

        let resolver = ArtifactResolver::new(&store, None);

        let config = OperatorConfig::default();
        let operator = OperatorAgent::new(config);

        // 创建包含占位符的工具调用
        let tool_call = crate::types::ToolCall {
            id: "test-1".to_string(),
            typ: "function".to_string(),
            function: crate::types::FunctionCall {
                name: "run_command".to_string(),
                arguments: r#"{"command": "g++", "args": ["{artifact:main.cpp}", "-o", "main"]}"#
                    .to_string(),
            },
        };

        // 注入路径
        let injected = operator.inject_artifact_paths_into_tool_call(&tool_call, &resolver);

        // 验证占位符被替换
        assert!(
            injected
                .function
                .arguments
                .contains("/workspace/src/main.cpp")
        );
        assert!(!injected.function.arguments.contains("{artifact:main.cpp}"));
    }

    #[test]
    fn test_inject_paths_into_value_nested() {
        let mut store = ArtifactStore::new();
        // 使用 BuildArtifactKind::SourceFile 以便 resolve_source_file 能找到
        store.put(
            Artifact::new(
                "2",
                "test.cpp",
                ArtifactKind::BuildArtifact(
                    crate::agent::hierarchy::task::BuildArtifactKind::SourceFile,
                ),
                "goal_2",
            )
            .with_path("/home/user/test.cpp"),
        );

        let resolver = ArtifactResolver::new(&store, None);

        // 测试嵌套对象
        let mut value = serde_json::json!({
            "source": "{artifact:test.cpp}",
            "options": {
                "input": "{artifact:test.cpp}"
            }
        });

        let modified = OperatorAgent::inject_paths_into_value(&mut value, &resolver);

        assert!(modified);
        assert_eq!(value["source"], "/home/user/test.cpp");
        assert_eq!(value["options"]["input"], "/home/user/test.cpp");
    }
}
