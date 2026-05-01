//! `OperatorAgent` 实现：简化执行路径、LLM 调用、产物上下文、注入与工具结果分析。

use std::time::Instant;

use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts};
use crate::types::{Message, MessageContent};

use super::super::artifact_resolver::ArtifactResolver;
use super::super::task::{SubGoal, TaskResult, TaskStatus};
use super::OperatorAgent;
use super::state::{ReactState, SubgoalPhase, ToolExecutionOutcome};
use super::types::{OperatorConfig, OperatorError};

impl OperatorAgent {
    pub fn new(config: OperatorConfig) -> Self {
        Self { config }
    }

    pub async fn execute(&self, goal: &SubGoal) -> Result<TaskResult, OperatorError> {
        let start_time = Instant::now();

        log::info!(
            target: "crabmate",
            "[HIERARCHICAL] Operator (simple): goal_id={} desc={}",
            goal.goal_id,
            super::text::truncate_goal(&goal.description)
        );

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        Ok(TaskResult {
            task_id: goal.goal_id.clone(),
            status: TaskStatus::Completed,
            output: Some(format!("Completed: {} (simple mode)", goal.description)),
            error: None,
            artifacts: Vec::new(),
            duration_ms: start_time.elapsed().as_millis() as u64,
            tools_invoked: Vec::new(),
        })
    }

    pub(super) async fn call_llm(
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

    pub(super) fn analyze_tool_execution(
        &self,
        result: &super::super::tool_executor::ToolExecutionResult,
        goal: &SubGoal,
    ) -> ToolExecutionOutcome {
        super::operator_tool_analysis::analyze_operator_tool_execution(result, goal)
    }

    pub(super) fn build_context_with_artifacts(
        &self,
        goal: &SubGoal,
        extra_context: Option<&str>,
        resolver: &ArtifactResolver<'_>,
    ) -> Option<String> {
        let mut parts = Vec::new();

        if let Some(ctx) = extra_context {
            parts.push(ctx.to_string());
        }

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

    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.config.allowed_tools.is_empty()
            || self
                .config
                .allowed_tools
                .iter()
                .any(|t| t == tool_name || t == "*")
    }

    pub(crate) fn inject_artifact_paths_into_tool_call(
        &self,
        tool_call: &crate::types::ToolCall,
        resolver: &ArtifactResolver<'_>,
    ) -> crate::types::ToolCall {
        let mut modified_call = tool_call.clone();

        if let Ok(mut args) =
            serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
        {
            let modified = super::inject::inject_paths_into_value(&mut args, resolver);

            if modified && let Ok(new_args) = serde_json::to_string(&args) {
                modified_call.function.arguments = new_args;
                log::info!(
                    target: "crabmate",
                    "[HIERARCHICAL] Operator: injected artifact paths into tool={}",
                    tool_call.function.name
                );
            }
        }

        modified_call
    }

    pub(super) fn build_output_summary(&self, state: &ReactState) -> String {
        format!(
            "Completed {} iterations with {} observations (phase={:?}, stagnant_rounds={})",
            state.iteration,
            state.observations.len(),
            state.phase,
            state.progress.rounds_without_progress
        )
    }

    pub(super) async fn emit_convergence_timeline(
        &self,
        goal_id: &str,
        phase: SubgoalPhase,
        iteration: usize,
        error_count: Option<usize>,
        stagnant_rounds: usize,
        first_error: Option<&str>,
    ) {
        let Some(ref sse_out) = self.config.sse_out else {
            return;
        };
        let phase_label = match phase {
            SubgoalPhase::Diagnose => "诊断",
            SubgoalPhase::ApplyFix => "修复",
            SubgoalPhase::Verify => "验证",
            SubgoalPhase::Escalate => "升级",
        };
        let mut detail = format!(
            "- 阶段：{}\n- 轮次：{}\n- 无进展轮次：{}",
            phase_label, iteration, stagnant_rounds
        );
        if let Some(n) = error_count {
            detail.push_str(&format!("\n- 错误数：{}", n));
        }
        if let Some(sig) = first_error
            && !sig.trim().is_empty()
        {
            detail.push_str(&format!("\n- 首错：{}", super::text::truncate_output(sig)));
        }
        let payload = crate::sse::encode_message(crate::sse::SsePayload::TimelineLog {
            log: crate::sse::protocol::TimelineLogBody {
                kind: "hierarchical_subgoal".to_string(),
                title: format!("子目标 `{}`", goal_id),
                detail: Some(detail),
            },
        });
        let _ =
            crate::sse::send_string_logged(sse_out, payload, "hierarchical::convergence_timeline")
                .await;
    }

    pub fn inject_paths_into_value(
        value: &mut serde_json::Value,
        resolver: &ArtifactResolver<'_>,
    ) -> bool {
        super::inject::inject_paths_into_value(value, resolver)
    }
}

/// 将本轮 LLM 答复记入 ReAct 历史。
pub(crate) fn assistant_message_for_operator_history(
    response: &Message,
    content: Option<MessageContent>,
    tool_calls: Option<Vec<crate::types::ToolCall>>,
) -> Message {
    Message {
        role: "assistant".to_string(),
        content,
        reasoning_content: response.reasoning_content.clone(),
        reasoning_details: response.reasoning_details.clone(),
        tool_calls,
        name: None,
        tool_call_id: None,
    }
}
