//! [`super::super::types::OperatorAgent::process_single_tool_call`]：SSE / 去重 / 重复命令检测拆分以降低圈复杂度。
//!
//! 由 [`super`]（`react_loop.rs`）末尾 `mod process_tool` 引入。

use std::time::Instant;

use crate::agent::hierarchy::artifact_resolver::ArtifactResolver;
use crate::agent::hierarchy::operator::state::ReactState;
use crate::agent::hierarchy::task::{SubGoal, TaskResult, TaskStatus};
use crate::agent::hierarchy::tool_executor::{ToolExecutionResult, ToolExecutor};
use crate::types::{Message, MessageContent};

impl super::super::types::OperatorAgent {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn process_single_tool_call(
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
            self.push_tool_not_allowed_observation(state, tool_call, tool_name);
            return None;
        }

        self.emit_operator_tool_call_sse(goal, tool_call).await;

        let injected_tool_call = if let Some(resolver) = resolver {
            self.inject_artifact_paths_into_tool_call(tool_call, resolver)
        } else {
            tool_call.clone()
        };

        let dedupe_key = Self::lightweight_dedupe_signature_for_run_command(
            &injected_tool_call.function.name,
            &injected_tool_call.function.arguments,
        );

        let (result, reused_lightweight_result) = Self::execute_with_lightweight_cache(
            tool_executor,
            &injected_tool_call,
            state,
            dedupe_key.as_ref(),
        )
        .await;

        state.tool_names_chron.push(result.tool_name.clone());
        state.tools_used.insert(result.tool_name.clone());
        Self::apply_lightweight_run_command_cache_update(
            &mut state.lightweight_command_cache,
            reused_lightweight_result,
            &dedupe_key,
            &result,
        );

        if let Some(new_dir) = crate::agent::hierarchy::operator::inject::detect_working_dir_change(
            &injected_tool_call,
            &result,
        ) {
            state.current_working_dir = Some(new_dir);
        }

        self.emit_operator_tool_result_sse(goal, tool_call, &result)
            .await;

        if let Some(done) = self.maybe_abort_duplicate_command(
            goal,
            state,
            tool_call,
            &result,
            reused_lightweight_result,
            start_time,
        ) {
            return Some(done);
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

    fn push_tool_not_allowed_observation(
        &self,
        state: &mut ReactState,
        tool_call: &crate::types::ToolCall,
        tool_name: &str,
    ) {
        state
            .observations
            .push(format!("Tool {} is not allowed", tool_name));
        state.messages.push(Message {
            role: "tool".to_string(),
            content: Some(MessageContent::Text(format!(
                "Error: Tool {} is not allowed. Available tools: {:?}",
                tool_name, self.config.policy.allowed_tools
            ))),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some(tool_call.id.clone()),
        });
    }

    async fn emit_operator_tool_call_sse(
        &self,
        goal: &SubGoal,
        tool_call: &crate::types::ToolCall,
    ) {
        let Some(ref sse_out) = self.config.runtime.sse_out else {
            return;
        };
        let tool_name = &tool_call.function.name;
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
        let _ =
            crate::sse::send_string_logged(sse_out, encoded, "hierarchical::operator_tool_call")
                .await;
    }

    async fn execute_with_lightweight_cache(
        tool_executor: &ToolExecutor,
        injected_tool_call: &crate::types::ToolCall,
        state: &ReactState,
        dedupe_key: Option<&String>,
    ) -> (ToolExecutionResult, bool) {
        let mut reused_lightweight_result = false;
        let result = if let Some(key) = dedupe_key {
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
                tool_executor.execute_tool_call(injected_tool_call).await
            }
        } else {
            tool_executor.execute_tool_call(injected_tool_call).await
        };
        (result, reused_lightweight_result)
    }

    async fn emit_operator_tool_result_sse(
        &self,
        goal: &SubGoal,
        tool_call: &crate::types::ToolCall,
        result: &ToolExecutionResult,
    ) {
        let Some(ref sse_out) = self.config.runtime.sse_out else {
            return;
        };
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
        let _ =
            crate::sse::send_string_logged(sse_out, encoded, "hierarchical::operator_tool_result")
                .await;
    }

    fn maybe_abort_duplicate_command(
        &self,
        goal: &SubGoal,
        state: &mut ReactState,
        tool_call: &crate::types::ToolCall,
        result: &ToolExecutionResult,
        reused_lightweight_result: bool,
        start_time: Instant,
    ) -> Option<TaskResult> {
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
        None
    }
}
