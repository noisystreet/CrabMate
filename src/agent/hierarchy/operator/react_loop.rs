//! Operator 子目标 ReAct 主循环（LLM + 工具 + SSE）。

use std::time::Instant;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::types::{Message, MessageContent};

use super::super::artifact_resolver::ArtifactResolver;
use super::super::task::{SubGoal, TaskResult, TaskStatus};
use super::super::tool_executor::ToolExecutor;
use super::state::{ConvergenceProgress, ReactState, SubgoalPhase, ToolExecutionOutcome};
use super::types::{CompileErrorType, OperatorError};

#[allow(dead_code, clippy::too_many_arguments)]
impl super::types::OperatorAgent {
    pub(super) fn get_lightweight_cached_run_command_result(
        cache: &std::collections::HashMap<String, super::super::tool_executor::ToolExecutionResult>,
        tool_name: &str,
        tool_args_json: &str,
    ) -> Option<super::super::tool_executor::ToolExecutionResult> {
        let key = Self::lightweight_dedupe_signature_for_run_command(tool_name, tool_args_json)?;
        cache.get(&key).cloned()
    }

    pub(super) fn lightweight_dedupe_signature_for_run_command(
        tool_name: &str,
        tool_args_json: &str,
    ) -> Option<String> {
        if tool_name != "run_command" {
            return None;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(tool_args_json) else {
            return None;
        };
        let command = args.get("command").and_then(|v| v.as_str())?;
        let raw_args = args.get("args").and_then(|v| v.as_array())?;
        let argv: Vec<&str> = raw_args.iter().filter_map(|v| v.as_str()).collect();
        match command.trim() {
            "cat" if argv.len() == 1 => Some(format!("run_command:cat:{}", argv[0].trim())),
            "ls" => {
                let target = argv
                    .iter()
                    .rev()
                    .find(|a| !a.trim().starts_with('-'))
                    .map(|s| s.trim())
                    .unwrap_or(".");
                Some(format!("run_command:ls:{target}"))
            }
            _ => None,
        }
    }

    pub(super) fn is_successful_build_executable_run_command(
        goal: &SubGoal,
        tool_name: &str,
        tool_args_json: &str,
        tool_success: bool,
    ) -> bool {
        if !tool_success || tool_name != "run_command" {
            return false;
        }
        if !super::super::goal_verifier::is_run_executable_subgoal(goal) {
            return false;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(tool_args_json) else {
            return false;
        };
        let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
            return false;
        };
        let cmd = command.trim();
        cmd.starts_with("./build/") || cmd.starts_with("build/")
    }

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

        log::info!(target: "crabmate", "[HIERARCHICAL] Operator (react): goal_id={} desc={}", goal.goal_id, super::text::truncate_goal(&goal.description));

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
            lightweight_command_cache: std::collections::HashMap::new(),
            tools_used: std::collections::HashSet::new(),
            tool_names_chron: Vec::new(),
            dynamic_decomposition_count: 0,
            phase: SubgoalPhase::Diagnose,
            progress: ConvergenceProgress::default(),
            last_reported_phase: None,
        };
        let convergence_goal = super::compile::is_convergence_compile_fix_goal(goal);

        // 构建初始系统提示（传入当前工作目录）
        let system_prompt = super::prompt::build_system_prompt(
            &self.config,
            goal,
            state.current_working_dir.as_deref(),
        );
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
            if convergence_goal {
                if state.last_reported_phase != Some(state.phase) {
                    self.emit_convergence_timeline(
                        &goal.goal_id,
                        state.phase,
                        state.iteration,
                        state.progress.last_error_count,
                        state.progress.rounds_without_progress,
                        state.progress.last_first_error_signature.as_deref(),
                    )
                    .await;
                    state.last_reported_phase = Some(state.phase);
                }
                state.observations.push(format!(
                    "Phase {:?} (iteration {})",
                    state.phase, state.iteration
                ));
            }

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
                    tools_invoked: state.tool_names_chron.clone(),
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
                state
                    .messages
                    .push(super::agent_impl::assistant_message_for_operator_history(
                        &response,
                        response.content.clone(),
                        Some(tool_calls.clone()),
                    ));

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
                                    goal_id: Some(goal.goal_id.clone()),
                                    tool_call_id: Some(tool_call.id.clone()),
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

                    let dedupe_key = Self::lightweight_dedupe_signature_for_run_command(
                        &injected_tool_call.function.name,
                        &injected_tool_call.function.arguments,
                    );
                    let mut reused_lightweight_result = false;
                    // 执行真实工具（使用注入后的参数）；对同一子目标内重复 cat/ls 复用上次结果，避免无效重复执行
                    let result = if let Some(key) = dedupe_key.as_ref() {
                        if let Some(cached) = Self::get_lightweight_cached_run_command_result(
                            &state.lightweight_command_cache,
                            &injected_tool_call.function.name,
                            &injected_tool_call.function.arguments,
                        ) {
                            reused_lightweight_result = true;
                            log::info!(
                                target: "crabmate",
                                "[HIERARCHICAL] Operator: lightweight dedupe hit for {}",
                                key
                            );
                            cached
                        } else {
                            tool_executor.execute_tool_call(&injected_tool_call).await
                        }
                    } else {
                        tool_executor.execute_tool_call(&injected_tool_call).await
                    };

                    log::info!(
                        target: "crabmate",
                        "[HIERARCHICAL] Operator: tool={} success={} output_len={}",
                        result.tool_name,
                        result.success,
                        result.output.len()
                    );
                    state.tool_names_chron.push(result.tool_name.clone());
                    state.tools_used.insert(result.tool_name.clone());
                    if !reused_lightweight_result
                        && result.success
                        && let Some(key) = dedupe_key.as_ref()
                    {
                        state
                            .lightweight_command_cache
                            .insert(key.clone(), result.clone());
                    }

                    // 检测工作目录变化（从工具参数或输出中提取）
                    if let Some(new_dir) =
                        super::inject::detect_working_dir_change(&injected_tool_call, &result)
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
                                    goal_id: Some(goal.goal_id.clone()),
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
                    if !reused_lightweight_result
                        && state.recent_commands.contains(&command_signature)
                    {
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
                                tools_invoked: state.tool_names_chron.clone(),
                            });
                        }
                    } else if !reused_lightweight_result {
                        // 新命令，重置重复计数
                        state.duplicate_command_count = 0;
                    }

                    // 保持最近 5 条命令历史
                    if !reused_lightweight_result {
                        state.recent_commands.push(command_signature);
                        if state.recent_commands.len() > 5 {
                            state.recent_commands.remove(0);
                        }
                    }

                    // 分析工具执行结果，检查是否表示任务已完成
                    let execution_outcome = self.analyze_tool_execution(&result, goal);

                    // 记录观察结果
                    let observation = if result.success {
                        format!(
                            "Tool {} executed successfully: {}",
                            result.tool_name,
                            super::text::truncate_for_subgoal_trace(
                                &result.output,
                                &result.tool_name
                            )
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
                        && super::compile::is_compile_command(
                            &result.tool_name,
                            &tool_call.function.arguments,
                        )
                        && let Some(error_info) =
                            super::compile::analyze_compile_error(&result.output)
                        && error_info.retryable
                    {
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: detected retryable compile error: {:?}",
                            error_info.error_type
                        );
                        error_recovery_hint = Some(
                            super::compile::build_compile_error_recovery_hint(&error_info),
                        );
                        current_error_type = Some(error_info.error_type.clone());
                    }

                    if convergence_goal
                        && super::compile::is_compile_command(
                            &result.tool_name,
                            &tool_call.function.arguments,
                        )
                    {
                        state.phase = SubgoalPhase::Verify;
                        if let Some(metrics) =
                            super::compile::parse_compile_error_metrics(&result.output)
                        {
                            let improved = state.progress.last_error_count.is_none_or(|prev| {
                                metrics.error_count < prev
                                    || state
                                        .progress
                                        .last_first_error_signature
                                        .as_deref()
                                        .is_some_and(|sig| sig != metrics.first_error_signature)
                            });
                            if improved {
                                state.progress.rounds_without_progress = 0;
                            } else {
                                state.progress.rounds_without_progress += 1;
                            }
                            state.progress.last_error_count = Some(metrics.error_count);
                            state.progress.last_first_error_signature =
                                Some(metrics.first_error_signature.clone());
                            state.observations.push(format!(
                                "Convergence metrics: errors={} first='{}' stagnant_rounds={}",
                                metrics.error_count,
                                super::text::truncate_output(&metrics.first_error_signature),
                                state.progress.rounds_without_progress
                            ));
                            if metrics.error_count > 0 {
                                state.phase = SubgoalPhase::ApplyFix;
                            }
                            if state.progress.rounds_without_progress >= 2 {
                                state.phase = SubgoalPhase::Escalate;
                                let reason = format!(
                                    "连续 {} 轮无进展（error_count={}, first_error='{}'），建议升级处理策略",
                                    state.progress.rounds_without_progress,
                                    metrics.error_count,
                                    super::text::truncate_output(&metrics.first_error_signature)
                                );
                                return Ok(TaskResult {
                                    task_id: goal.goal_id.clone(),
                                    status: TaskStatus::Failed {
                                        reason: reason.clone(),
                                    },
                                    output: Some(self.build_output_summary(&state)),
                                    error: Some(reason),
                                    artifacts: Vec::new(),
                                    duration_ms: start_time.elapsed().as_millis() as u64,
                                    tools_invoked: state.tool_names_chron.clone(),
                                });
                            }
                            if state.last_reported_phase != Some(state.phase) {
                                self.emit_convergence_timeline(
                                    &goal.goal_id,
                                    state.phase,
                                    state.iteration,
                                    state.progress.last_error_count,
                                    state.progress.rounds_without_progress,
                                    state.progress.last_first_error_signature.as_deref(),
                                )
                                .await;
                                state.last_reported_phase = Some(state.phase);
                            }
                        }
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
                                tools_invoked: state.tool_names_chron.clone(),
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
                    // 收敛策略：若已成功执行 build 目录下可执行文件，立即结束本子目标，
                    // 防止继续进入“确认回合”重复执行 ls/cat/cmake --build 等。
                    if !state.task_completed
                        && Self::is_successful_build_executable_run_command(
                            goal,
                            &result.tool_name,
                            &tool_call.function.arguments,
                            result.success,
                        )
                    {
                        let reason = "Built executable run_command succeeded".to_string();
                        log::info!(
                            target: "crabmate",
                            "[HIERARCHICAL] Operator: early convergence after successful executable run: tool={}",
                            result.tool_name
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
                                super::super::tool_executor::ExtractedArtifactKind::SourceFile => {
                                    // 尝试读取源文件内容并记录
                                    if let Ok(content) = std::fs::read_to_string(&artifact.path) {
                                        build_state.record_source_file(&artifact.path, &content);
                                    }
                                }
                                super::super::tool_executor::ExtractedArtifactKind::ObjectFile => {
                                    build_state.add_object_file(artifact.path.clone());
                                }
                                super::super::tool_executor::ExtractedArtifactKind::Executable => {
                                    build_state.add_executable(artifact.path.clone());
                                }
                                super::super::tool_executor::ExtractedArtifactKind::StaticLibrary => {
                                    build_state.add_static_library(artifact.path.clone());
                                }
                                super::super::tool_executor::ExtractedArtifactKind::DynamicLibrary => {
                                    build_state.add_dynamic_library(artifact.path.clone());
                                }
                                super::super::tool_executor::ExtractedArtifactKind::BuildDirectory => {
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
                        let trace = state.observations.join("\n");
                        return Ok(TaskResult {
                            task_id: goal.goal_id.clone(),
                            status: TaskStatus::Completed,
                            output: Some(format!(
                                "Task completed: {}\n\n[subgoal_tool_trace]\n{}",
                                reason, trace
                            )),
                            error: None,
                            artifacts: Vec::new(),
                            duration_ms: start_time.elapsed().as_millis() as u64,
                            tools_invoked: state.tool_names_chron.clone(),
                        });
                    }

                    // 动态子目标分解检查
                    if self.config.enable_dynamic_decomposition
                        && state.dynamic_decomposition_count == 0 // 只触发一次
                        && state.iteration >= 8
                    // 至少执行了5轮
                    {
                        let decomposer = super::super::dynamic_decomposer::DynamicDecomposer::new();
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
                                tools_invoked: state.tool_names_chron.clone(),
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
                                .push(format!("Final: {}", super::text::truncate_output(&text)));
                            // 仅在未开启 CM_WEB_RAW_ASSISTANT_OUTPUT 时剥离思维链标签
                            let mut output =
                                if crate::web::web_ui_env::web_raw_assistant_output_env() {
                                    text.clone()
                                } else {
                                    super::text::strip_thinking_tags(&text)
                                };
                            // 与「工具触发提前结束」路径一致：验收依赖 `[subgoal_tool_trace]` 内的 `Tool run_command…` 行；
                            // 若仅因模型无工具收尾而走到此处，必须把本轮已执行的工具观察一并写入，否则 GoalVerifier 会误判。
                            let trace = state.observations.join("\n");
                            if !trace.trim().is_empty() {
                                output.push_str("\n\n[subgoal_tool_trace]\n");
                                output.push_str(&trace);
                            }
                            return Ok(TaskResult {
                                task_id: goal.goal_id.clone(),
                                status: TaskStatus::Completed,
                                output: Some(output),
                                error: None,
                                artifacts: Vec::new(),
                                duration_ms: start_time.elapsed().as_millis() as u64,
                                tools_invoked: state.tool_names_chron.clone(),
                            });
                        } else {
                            // LLM 可能需要继续，将回复作为观察并添加到消息历史
                            state.observations.push(format!(
                                "LLM response: {}",
                                super::text::truncate_output(&text)
                            ));
                            // 重要：将 LLM 回复添加到 messages，否则上下文会丢失
                            state.messages.push(
                                super::agent_impl::assistant_message_for_operator_history(
                                    &response,
                                    Some(MessageContent::Text(text.clone())),
                                    None,
                                ),
                            );
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
}
