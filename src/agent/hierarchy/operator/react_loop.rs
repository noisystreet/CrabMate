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

            if let Some(result) = self
                .process_react_round(
                    goal,
                    tool_executor,
                    resolver.as_ref(),
                    &mut state,
                    &response,
                    convergence_goal,
                    start_time,
                )
                .await
            {
                return Ok(result);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_react_round(
        &self,
        goal: &SubGoal,
        tool_executor: &ToolExecutor,
        resolver: Option<&ArtifactResolver<'_>>,
        state: &mut ReactState,
        response: &Message,
        convergence_goal: bool,
        start_time: Instant,
    ) -> Option<TaskResult> {
        if let Some(tool_calls) = &response.tool_calls {
            state
                .messages
                .push(super::agent_impl::assistant_message_for_operator_history(
                    response,
                    response.content.clone(),
                    Some(tool_calls.clone()),
                ));
            for tool_call in tool_calls {
                if let Some(done) = self
                    .process_single_tool_call(
                        goal,
                        tool_executor,
                        resolver,
                        state,
                        tool_call,
                        convergence_goal,
                        start_time,
                    )
                    .await
                {
                    return Some(done);
                }
            }
            return None;
        }
        self.process_non_tool_response(goal, state, response, start_time)
    }

    fn process_non_tool_response(
        &self,
        goal: &SubGoal,
        state: &mut ReactState,
        response: &Message,
        start_time: Instant,
    ) -> Option<TaskResult> {
        let Some(content) = &response.content else {
            log::warn!(target: "crabmate", "[HIERARCHICAL] Operator: LLM returned no content, adding prompt to continue");
            state
                .messages
                .push(Message::user_only("请继续执行任务。".to_string()));
            return None;
        };
        let text = crate::types::message_content_as_str(&Some(content.clone()))
            .unwrap_or("")
            .to_string();
        if text.is_empty() {
            log::warn!(target: "crabmate", "[HIERARCHICAL] Operator: LLM returned empty content, adding prompt to continue");
            state
                .messages
                .push(Message::user_only("请继续执行任务。".to_string()));
            return None;
        }
        if text.contains("完成") || text.contains("finished") || text.contains("done") {
            state
                .observations
                .push(format!("Final: {}", super::text::truncate_output(&text)));
            let mut output = if crate::web::web_ui_env::web_raw_assistant_output_env() {
                text.clone()
            } else {
                super::text::strip_thinking_tags(&text)
            };
            let trace = state.observations.join("\n");
            if !trace.trim().is_empty() {
                output.push_str("\n\n[subgoal_tool_trace]\n");
                output.push_str(&trace);
            }
            return Some(TaskResult {
                task_id: goal.goal_id.clone(),
                status: TaskStatus::Completed,
                output: Some(output),
                error: None,
                artifacts: Vec::new(),
                duration_ms: start_time.elapsed().as_millis() as u64,
                tools_invoked: state.tool_names_chron.clone(),
            });
        }
        state.observations.push(format!(
            "LLM response: {}",
            super::text::truncate_output(&text)
        ));
        state
            .messages
            .push(super::agent_impl::assistant_message_for_operator_history(
                response,
                Some(MessageContent::Text(text)),
                None,
            ));
        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_single_tool_call(
        &self,
        goal: &SubGoal,
        tool_executor: &ToolExecutor,
        resolver: Option<&ArtifactResolver<'_>>,
        state: &mut ReactState,
        tool_call: &crate::types::ToolCall,
        convergence_goal: bool,
        start_time: Instant,
    ) -> Option<TaskResult> {
        let tool_name = &tool_call.function.name;
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
            return None;
        }
        if let Some(ref sse_out) = self.config.sse_out {
            let args = &tool_call.function.arguments;
            let summary = crate::tools::summarize_tool_call(tool_name, args)
                .unwrap_or_else(|| format!("tool: {tool_name}"));
            let encoded = crate::sse::encode_message(crate::sse::SsePayload::ToolCall {
                tool_call: crate::sse::protocol::ToolCallSummary {
                    name: tool_name.clone(),
                    summary,
                    goal_id: Some(goal.goal_id.clone()),
                    tool_call_id: Some(tool_call.id.clone()),
                    arguments_preview: Some(crate::redact::tool_arguments_preview_for_sse(args)),
                    arguments: Some(crate::redact::tool_arguments_redacted_for_sse(args)),
                },
            });
            let _ = crate::sse::send_string_logged(
                sse_out,
                encoded,
                "hierarchical::operator_tool_call",
            )
            .await;
        }
        let injected_tool_call = if let Some(resolver) = resolver {
            self.inject_artifact_paths_into_tool_call(tool_call, resolver)
        } else {
            tool_call.clone()
        };
        let dedupe_key = Self::lightweight_dedupe_signature_for_run_command(
            &injected_tool_call.function.name,
            &injected_tool_call.function.arguments,
        );
        let mut reused_lightweight_result = false;
        let result = if let Some(key) = dedupe_key.as_ref() {
            if let Some(cached) = Self::get_lightweight_cached_run_command_result(
                &state.lightweight_command_cache,
                &injected_tool_call.function.name,
                &injected_tool_call.function.arguments,
            ) {
                reused_lightweight_result = true;
                log::info!(target: "crabmate", "[HIERARCHICAL] Operator: lightweight dedupe hit for {}", key);
                cached
            } else {
                tool_executor.execute_tool_call(&injected_tool_call).await
            }
        } else {
            tool_executor.execute_tool_call(&injected_tool_call).await
        };
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
        if let Some(new_dir) =
            super::inject::detect_working_dir_change(&injected_tool_call, &result)
        {
            state.current_working_dir = Some(new_dir);
        }
        if let Some(ref sse_out) = self.config.sse_out {
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
            let encoded = crate::sse::encode_message(crate::sse::SsePayload::ToolResult {
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
                    structured_preview: None,
                },
            });
            let _ = crate::sse::send_string_logged(
                sse_out,
                encoded,
                "hierarchical::operator_tool_result",
            )
            .await;
        }
        let command_signature = format!("{}:{}", result.tool_name, tool_call.function.arguments);
        if !reused_lightweight_result && state.recent_commands.contains(&command_signature) {
            state.duplicate_command_count += 1;
            if state.duplicate_command_count >= 2 {
                return Some(TaskResult {
                    task_id: goal.goal_id.clone(),
                    status: TaskStatus::Failed {
                        reason: format!(
                            "检测到重复执行同一命令 {} 次，可能陷入循环。请检查任务逻辑。",
                            state.duplicate_command_count + 1
                        ),
                    },
                    output: Some(self.build_output_summary(state)),
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
            state.duplicate_command_count = 0;
        }
        if !reused_lightweight_result {
            state.recent_commands.push(command_signature);
            if state.recent_commands.len() > 5 {
                state.recent_commands.remove(0);
            }
        }
        let execution_outcome = self.analyze_tool_execution(&result, goal);
        self.process_single_tool_call_after_execute(
            goal,
            state,
            tool_call,
            &result,
            convergence_goal,
            start_time,
            execution_outcome,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_single_tool_call_after_execute(
        &self,
        goal: &SubGoal,
        state: &mut ReactState,
        tool_call: &crate::types::ToolCall,
        result: &super::super::tool_executor::ToolExecutionResult,
        convergence_goal: bool,
        start_time: Instant,
        execution_outcome: ToolExecutionOutcome,
    ) -> Option<TaskResult> {
        let observation = if result.success {
            format!(
                "Tool {} executed successfully: {}",
                result.tool_name,
                super::text::truncate_for_subgoal_trace(&result.output, &result.tool_name)
            )
        } else {
            format!("Tool {} failed: {}", result.tool_name, result.output)
        };
        state.observations.push(observation);
        let mut error_recovery_hint = None;
        let mut current_error_type: Option<CompileErrorType> = None;
        if !result.success
            && self.config.enable_compile_error_recovery
            && super::compile::is_compile_command(&result.tool_name, &tool_call.function.arguments)
            && let Some(error_info) = super::compile::analyze_compile_error(&result.output)
            && error_info.retryable
        {
            error_recovery_hint = Some(super::compile::build_compile_error_recovery_hint(
                &error_info,
            ));
            current_error_type = Some(error_info.error_type.clone());
        }
        if convergence_goal
            && super::compile::is_compile_command(&result.tool_name, &tool_call.function.arguments)
        {
            state.phase = SubgoalPhase::Verify;
            if let Some(done) = self
                .handle_convergence_metrics(goal, state, &result.output, start_time)
                .await
            {
                return Some(done);
            }
        }
        if !result.success {
            state.consecutive_failures += 1;
            state.last_failed_tool = Some(result.tool_name.clone());
            state.last_error_type = current_error_type;
            if state.consecutive_failures >= 3 {
                return Some(TaskResult {
                    task_id: goal.goal_id.clone(),
                    status: TaskStatus::Failed {
                        reason: format!(
                            "连续 {} 次执行失败。请检查工作目录和命令参数是否正确。",
                            state.consecutive_failures
                        ),
                    },
                    output: Some("连续失败，提前终止".to_string()),
                    error: Some("连续失败，提前终止".to_string()),
                    artifacts: Vec::new(),
                    duration_ms: start_time.elapsed().as_millis() as u64,
                    tools_invoked: state.tool_names_chron.clone(),
                });
            }
        } else {
            state.consecutive_failures = 0;
            state.last_failed_tool = None;
            state.last_error_type = None;
        }
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
        if let ToolExecutionOutcome::TaskCompleted { reason } = execution_outcome {
            state.task_completed = true;
            state.completion_reason = Some(reason);
        }
        if !state.task_completed
            && Self::is_successful_build_executable_run_command(
                goal,
                &result.tool_name,
                &tool_call.function.arguments,
                result.success,
            )
        {
            state.task_completed = true;
            state.completion_reason = Some("Built executable run_command succeeded".to_string());
        }
        if state.task_completed {
            let reason = state.completion_reason.clone().unwrap_or_default();
            let trace = state.observations.join("\n");
            return Some(TaskResult {
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
        if let Some(ref build_state_arc) = self.config.build_state
            && let Ok(mut build_state) = build_state_arc.lock()
        {
            for artifact in &result.extracted_artifacts {
                match artifact.kind {
                    super::super::tool_executor::ExtractedArtifactKind::SourceFile => {
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
        if self.config.enable_dynamic_decomposition
            && state.dynamic_decomposition_count == 0
            && state.iteration >= 8
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
                let reason = assessment.reason.clone();
                return Some(TaskResult {
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
        None
    }

    async fn handle_convergence_metrics(
        &self,
        goal: &SubGoal,
        state: &mut ReactState,
        output: &str,
        start_time: Instant,
    ) -> Option<TaskResult> {
        let metrics = super::compile::parse_compile_error_metrics(output)?;
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
        state.progress.last_first_error_signature = Some(metrics.first_error_signature.clone());
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
            return Some(TaskResult {
                task_id: goal.goal_id.clone(),
                status: TaskStatus::Failed {
                    reason: reason.clone(),
                },
                output: Some(self.build_output_summary(state)),
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
        None
    }
}
