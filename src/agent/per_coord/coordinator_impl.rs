use crate::config::AgentConfig;
use crate::types::Message;
use serde_json::Value;

use super::workflow_reflection_controller::{self, WorkflowReflectionController};
use super::{
    AfterFinalAssistant, FinalPlanRequirementMode, PLAN_REWRITE_EXHAUSTED_SSE, PerCoordinator,
    PerCoordinatorInit, PlanRequirementSource, PreparedWorkflowExecute,
};
use super::{final_plan_gate, plan_artifact, plan_rewrite};

fn stop_output_to_string(stop_output: Option<Value>) -> String {
    let stop_v = stop_output.unwrap_or_else(|| {
        Value::String("workflow_execute 已停止（反思控制器拒绝继续执行）。".to_string())
    });
    match stop_v {
        Value::String(s) => s,
        v => v.to_string(),
    }
}

impl PerCoordinator {
    pub fn plan_rewrite_exhausted_sse_message() -> &'static str {
        PLAN_REWRITE_EXHAUSTED_SSE
    }

    /// 供 `/status` 等只读镜像：`after_final_assistant` 已递增后的重写次数。
    pub fn plan_rewrite_attempts_snapshot(&self) -> usize {
        self.counters.plan_rewrite_attempts
    }

    /// 本回合内已成功完成的分阶段补丁规划轮次数（与 [`Self::plan_rewrite_attempts_snapshot`] 独立）。
    pub fn staged_plan_patch_planner_rounds_snapshot(&self) -> usize {
        self.counters.staged_plan_patch_planner_rounds_completed
    }

    /// 配置中的 **`staged_plan_patch_max_attempts`**（与单步失败分支内补丁循环上界一致；**非** `plan_rewrite_max_attempts`）。
    pub fn staged_plan_patch_max_attempts_config_snapshot(&self) -> usize {
        self.staged_plan_patch_max_attempts_config
    }

    /// 配置中的规划重写上限（与 `plan_rewrite_max_attempts` 一致）。
    pub fn plan_rewrite_max_attempts_limit(&self) -> usize {
        self.plan_rewrite_max_attempts
    }

    /// 分阶段补丁规划轮成功合并 `steps` 后调用，递增与 **`plan_rewrite`** 独立的计数。
    pub(crate) fn record_staged_plan_patch_planner_round_completed(&mut self) {
        self.counters
            .record_staged_plan_patch_planner_round_completed();
    }

    /// 分阶段步级补丁耗尽等错误串尾部：标明与 **`plan_rewrite`** 独立的计数（供排障）。
    pub(crate) fn staged_plan_patch_vs_plan_rewrite_counters_footer(&self) -> String {
        format!(
            "\n\n[计数] 分阶段补丁规划已成功合并轮次={}（配置 `staged_plan_patch_max_attempts`={}，约束**本步失败分支**内尝试上界）；终答 `plan_rewrite` 已用次数={}/{}（**独立计数**，不计入上式）。",
            self.counters.staged_plan_patch_planner_rounds_completed,
            self.staged_plan_patch_max_attempts_config,
            self.counters.plan_rewrite_attempts,
            self.plan_rewrite_max_attempts
        )
    }

    pub(crate) fn plan_semantic_mismatch_rewrite_message_with_feedback(
        violation_codes: &[String],
        rationale: Option<&str>,
    ) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(
                plan_rewrite::user_text_semantic_mismatch_with_feedback(violation_codes, rationale)
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    /// 下一回模型若以非 `tool_calls` 结束，是否必须嵌入可解析的 `agent_reply_plan`（工作流反思路径下由工具结果置位）。
    pub fn require_plan_in_final_flag_snapshot(&self) -> bool {
        self.plan_requirement_source != PlanRequirementSource::None
    }

    pub fn new(init: PerCoordinatorInit) -> Self {
        let initial_source = match init.final_plan_policy {
            FinalPlanRequirementMode::Always => PlanRequirementSource::ConfigAlways,
            _ => PlanRequirementSource::None,
        };
        Self {
            reflection: WorkflowReflectionController::new(init.reflection_default_max_rounds),
            final_plan_policy: init.final_plan_policy,
            plan_rewrite_max_attempts: init.plan_rewrite_max_attempts.max(1),
            staged_plan_patch_max_attempts_config: init
                .staged_plan_patch_max_attempts_config
                .max(1),
            final_plan_require_strict_workflow_node_coverage: init
                .final_plan_require_strict_workflow_node_coverage,
            final_plan_semantic_check_enabled: init.final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: init
                .final_plan_semantic_check_max_non_readonly_tools,
            plan_requirement_source: initial_source,
            counters: super::per_turn_state::PerTurnCounters::new(),
            workflow_validate_cache: super::per_turn_state::WorkflowValidateLayerCache::new(),
            repeated_tool_failures: super::per_turn_state::RepeatedToolFailureMemo::new(),
        }
    }

    pub(crate) fn repeated_tool_failure_error_marker(
        &self,
        tool_name: &str,
        tool_args_json: &str,
    ) -> Option<&str> {
        self.repeated_tool_failures
            .repeated_tool_failure_error_marker(tool_name, tool_args_json)
    }

    pub(crate) fn mark_tool_failure_signature(
        &mut self,
        tool_name: &str,
        tool_args_json: &str,
        error_marker: String,
    ) {
        self.repeated_tool_failures.mark_tool_failure_signature(
            tool_name,
            tool_args_json,
            error_marker,
        );
    }

    pub(crate) fn repeated_tool_failure_family_marker(
        &self,
        tool_name: &str,
        failure_family: &str,
    ) -> Option<&str> {
        self.repeated_tool_failures
            .repeated_tool_failure_family_marker(tool_name, failure_family)
    }

    pub(crate) fn mark_tool_failure_family(
        &mut self,
        tool_name: &str,
        failure_family: &str,
        error_marker: String,
    ) {
        self.repeated_tool_failures.mark_tool_failure_family(
            tool_name,
            failure_family,
            error_marker,
        );
    }

    pub(crate) fn clear_tool_failure_signature(&mut self, tool_name: &str, tool_args_json: &str) {
        self.repeated_tool_failures
            .clear_tool_failure_signature(tool_name, tool_args_json);
    }

    pub(crate) fn clear_tool_failure_families_for_tool(&mut self, tool_name: &str) {
        self.repeated_tool_failures
            .clear_tool_failure_families_for_tool(tool_name);
    }

    /// `context_window` 在裁剪/摘要等**就地**改写 `messages` 后调用，避免 `layer_count` 缓存指向已删除的 `workflow_validate` 工具结果。
    pub fn invalidate_workflow_validate_layer_cache_after_context_mutation(&mut self) {
        self.workflow_validate_cache
            .invalidate_after_context_mutation();
    }

    pub(super) fn workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        self.workflow_validate_cache
            .workflow_validate_layer_need(messages)
    }

    /// 是否包含可解析的 `agent_reply_plan` v1 JSON（见 `plan_artifact`）。
    #[allow(dead_code)] // 供外部集成或调试保留；主路径用 `parse_agent_reply_plan_v1`
    pub fn content_has_plan(content: &str) -> bool {
        plan_artifact::content_has_valid_agent_reply_plan_v1(content)
    }

    /// 在已将 assistant 消息推入 `messages` 之后调用，根据是否需要「规划」段落决定下一步。
    pub fn after_final_assistant(
        &mut self,
        msg: &Message,
        messages: &[Message],
        cfg: &AgentConfig,
        workspace_is_set: bool,
    ) -> AfterFinalAssistant {
        final_plan_gate::after_final_assistant(self, msg, messages, cfg, workspace_is_set)
    }

    /// 对一次 `workflow_execute` 的 arguments 做反思决策、补丁与「要求最终带规划」标记更新。
    pub fn prepare_workflow_execute(&mut self, args_json: &str) -> PreparedWorkflowExecute {
        let decision = self.reflection.decide(args_json);
        let reflection_inject = decision.inject_instruction.clone();
        if matches!(
            self.final_plan_policy,
            FinalPlanRequirementMode::WorkflowReflection
        ) && let Some(v) = reflection_inject.as_ref()
            && v.get("instruction_type").and_then(|x| x.as_str())
                == Some(workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT)
        {
            self.plan_requirement_source = PlanRequirementSource::WorkflowReflection;
            log::info!(
                target: "crabmate::per",
                "prepare_workflow_execute event=require_final_plan_set reflection_stage_round={}",
                self.reflection.stage_round()
            );
        }
        let patched_args = match decision.workflow_args_patch.as_ref() {
            Some(patch) => workflow_reflection_controller::apply_workflow_patch(args_json, patch),
            None => args_json.to_string(),
        };
        let skipped_result = if decision.execute {
            String::new()
        } else {
            stop_output_to_string(decision.stop_output)
        };
        PreparedWorkflowExecute {
            patched_args,
            execute: decision.execute,
            skipped_result,
            reflection_inject,
        }
    }

    /// 追加 tool 消息以及可选的反思注入 user 消息（与原先两处 `run_agent_turn*` 行为一致）。
    pub fn append_tool_result_and_reflection(
        per_coord: &mut PerCoordinator,
        messages: &mut Vec<Message>,
        tool_call_id: String,
        result: String,
        reflection_inject: Option<Value>,
    ) {
        messages.push(Message {
            role: "tool".to_string(),
            content: Some(result.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some(tool_call_id),
        });
        if let Some(instruction) = reflection_inject {
            let instruction_str = match serde_json::to_string(&instruction) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        target: "crabmate::per",
                        "workflow_reflection_inject_serialize_failed error={}",
                        e
                    );
                    r#"{"instruction_type":"crabmate_reflection_serialize_failed","detail":"工作流反思指令无法序列化为 JSON，请根据上一轮工具结果继续。"}"#
                        .to_string()
                }
            };
            // 与 `prepare_workflow_execute` 一致：反思首轮要求终答含 `agent_reply_plan`；后续轮次注入也应保持同一策略，避免模型收到「须带规划」文案却未置位 PER。
            if matches!(
                per_coord.final_plan_policy,
                FinalPlanRequirementMode::WorkflowReflection
            ) && let Some(t) = instruction.get("instruction_type").and_then(|x| x.as_str())
                && (t == workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT
                    || t == "workflow_reflection_next")
            {
                per_coord.plan_requirement_source = PlanRequirementSource::WorkflowReflection;
                log::info!(
                    target: "crabmate::per",
                    "append_tool_result_and_reflection event=require_final_plan_set instruction_type={t} reflection_stage_round={}",
                    per_coord.reflection.stage_round()
                );
            }
            messages.push(Message {
                role: "user".to_string(),
                content: Some(instruction_str.into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            });
        }
        per_coord
            .workflow_validate_cache
            .refresh_after_messages_append(messages.len(), messages.as_slice());
    }
}
