//! 规划–执行–反思（PER）协调：workflow 反思状态机 + 最终回答中的「规划」校验。
//! Web 与 CLI 的 `run_agent_turn` 共用此层，避免双份维护。

use crate::config::AgentConfig;
use crate::tool_registry;
use crate::types::Message;

use super::plan_artifact;
use super::workflow_reflection_controller::{self, WorkflowReflectionController};
use serde_json::Value;

const PLAN_REWRITE_EXHAUSTED_SSE: &str =
    "结构化规划仍未满足要求（已达最大重写次数），已结束本轮；请调整需求后重试。";

/// 何时要求模型在**最终** assistant 正文中嵌入可解析的 `agent_reply_plan` v1（见 `plan_artifact`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalPlanRequirementMode {
    /// 从不强制；工作流反思仍会注入指令，但不触发 `after_final_assistant` 的重写循环。
    Never,
    /// 默认：仅当本轮工具路径注入了 [`workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`] 时，对随后的终答校验。
    #[default]
    WorkflowReflection,
    /// 每次模型以非 `tool_calls` 结束时均校验（实验性，易增加额外模型轮次）。
    Always,
}

/// 终答规划重写用尽时 SSE **`reason_code`** 的稳定子码（顶层 **`code`** 仍为 `plan_rewrite_exhausted`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRewriteExhaustedReason {
    /// 正文无可解析的 `agent_reply_plan` v1。
    PlanMissing,
    /// `steps` 条数低于最近一次 `workflow_validate` 的 `layer_count` 要求。
    PlanLayerCountMismatch,
    /// `workflow_node_id` 与最近工作流节点 id 集合不一致（子集校验失败）。
    PlanWorkflowNodeIdsInvalid,
    /// 严格模式下未覆盖全部工作流节点 id。
    PlanWorkflowNodeCoverageIncomplete,
    /// `workflow_validate_only` 后规划未与 **`nodes[].id` 一一绑定**（步数、`workflow_node_id` 必填或多重集合不一致）。
    PlanValidateOnlyNodeBindingMismatch,
    /// 侧向语义校验判定与工具结果矛盾且重写次数已用尽。
    PlanSemanticInconsistent,
    /// 未归类的用尽路径（防御性；主路径不应出现）。
    ExhaustedOther,
}

impl PlanRewriteExhaustedReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlanMissing => "plan_missing",
            Self::PlanLayerCountMismatch => "plan_layer_count_mismatch",
            Self::PlanWorkflowNodeIdsInvalid => "plan_workflow_node_ids_invalid",
            Self::PlanWorkflowNodeCoverageIncomplete => "plan_workflow_node_coverage_incomplete",
            Self::PlanValidateOnlyNodeBindingMismatch => "plan_validate_only_node_binding_mismatch",
            Self::PlanSemanticInconsistent => "plan_semantic_inconsistent",
            Self::ExhaustedOther => "plan_rewrite_exhausted_other",
        }
    }
}

fn classify_plan_rewrite_exhausted_reason(
    msg: &Message,
    messages: &[Message],
    layer_need: Option<usize>,
    apply_layer_semantics: bool,
    strict_workflow_node_coverage: bool,
) -> PlanRewriteExhaustedReason {
    let content = crate::types::message_content_as_str(&msg.content).unwrap_or("");
    let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1(content) else {
        return PlanRewriteExhaustedReason::PlanMissing;
    };
    let layers_ok = match layer_need {
        Some(n) if n > 0 && apply_layer_semantics => plan.steps.len() >= n,
        _ => true,
    };
    if !layers_ok {
        return PlanRewriteExhaustedReason::PlanLayerCountMismatch;
    }
    let wf_ids = last_workflow_tool_node_ids(messages);
    let workflow_subset_ok = match wf_ids.as_ref() {
        Some(ids) => plan_artifact::validate_plan_workflow_node_ids_subset(&plan, ids).is_ok(),
        None => true,
    };
    if !workflow_subset_ok {
        return PlanRewriteExhaustedReason::PlanWorkflowNodeIdsInvalid;
    }
    let workflow_cover_ok = if strict_workflow_node_coverage {
        match wf_ids.as_ref() {
            Some(ids) => {
                plan_artifact::validate_plan_covers_all_workflow_node_ids(&plan, ids).is_ok()
            }
            None => true,
        }
    } else {
        true
    };
    if !workflow_cover_ok {
        return PlanRewriteExhaustedReason::PlanWorkflowNodeCoverageIncomplete;
    }
    if apply_layer_semantics
        && let Some(ids) = last_workflow_validate_binding_plan_node_ids(messages)
        && !ids.is_empty()
        && plan_artifact::validate_plan_binds_workflow_validate_nodes(&plan, &ids).is_err()
    {
        return PlanRewriteExhaustedReason::PlanValidateOnlyNodeBindingMismatch;
    }
    PlanRewriteExhaustedReason::ExhaustedOther
}

impl FinalPlanRequirementMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "never" => Ok(Self::Never),
            "workflow_reflection" => Ok(Self::WorkflowReflection),
            "always" => Ok(Self::Always),
            _ => Err(format!(
                "未知 final_plan_requirement {:?}，应为 never / workflow_reflection / always",
                s.trim()
            )),
        }
    }
}

/// 标识 plan 需求的来源，使工作流反思与终答反思的交互点可审计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanRequirementSource {
    /// 无需求
    None,
    /// 来自 `FinalPlanRequirementMode::Always` 配置
    ConfigAlways,
    /// 来自工作流反思第一轮注入的 `INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT`
    WorkflowReflection,
}

fn plan_rewrite_user_text() -> String {
    format!(
        "你的最终回答缺少**结构化规划**。请在 content 中加入一段 Markdown 代码围栏（语言标记为 json），其内为合法 JSON，且必须满足：\n{}\n\n示例：\n```json\n{}\n```\n\n请直接重写本轮最终回答（可有其它说明文字，但须包含上述 JSON 围栏）。",
        plan_artifact::PLAN_V1_SCHEMA_RULES,
        plan_artifact::PLAN_V1_EXAMPLE_JSON
    )
}

/// 侧向语义校验失败时追加的 user 正文：自然语言说明 + 机器可读 `crabmate_plan_semantic_feedback` + 规划重写要求。
pub(crate) fn plan_rewrite_user_text_semantic_mismatch_with_feedback(
    violation_codes: &[String],
    rationale: Option<&str>,
) -> String {
    let codes_json: Vec<&str> = if violation_codes.is_empty() {
        vec!["semantic_mismatch_unspecified"]
    } else {
        violation_codes.iter().map(String::as_str).collect()
    };
    let rationale_json = rationale
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Null);
    let machine = serde_json::json!({
        "kind": "crabmate_plan_semantic_feedback",
        "version": 1,
        "violation_codes": codes_json,
        "rationale": rationale_json,
    });
    let machine_line = serde_json::to_string(&machine).unwrap_or_else(|_| {
        "{\"kind\":\"crabmate_plan_semantic_feedback\",\"version\":1,\"violation_codes\":[\"semantic_mismatch_unspecified\"],\"rationale\":null}".to_string()
    });
    format!(
        "侧向校验认为你的 **agent_reply_plan** 与**最近工具执行结果**存在明显矛盾。请根据下方 **violation_codes**（及可选 **rationale**）与**真实工具结果**修正规划 JSON 与说明文字。\n\n```json\n{}\n```\n\n{}",
        machine_line,
        plan_rewrite_user_text()
    )
}

/// 构造 `PerCoordinator` 的运行期参数（嵌入默认 + 热重载后由 `AgentConfig` 填充）。
#[derive(Debug, Clone)]
pub struct PerCoordinatorInit {
    pub reflection_default_max_rounds: usize,
    pub final_plan_policy: FinalPlanRequirementMode,
    pub plan_rewrite_max_attempts: usize,
    /// 为 true 时：若任一步填写 `workflow_node_id`，则须覆盖最近一次工作流工具结果中的**全部** `nodes[].id`。
    pub final_plan_require_strict_workflow_node_coverage: bool,
    /// 可选二次 LLM：对比规划 JSON 与最近工具摘要；默认 false。
    pub final_plan_semantic_check_enabled: bool,
    /// 语义校验摘要中最多收录的**非只读**工具条数（0 表示不收录写类工具正文）。
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
}

/// 模型返回最终文本（非 tool_calls）后，由协调层决定是结束本轮还是要求重写。
#[derive(Debug)]
pub enum AfterFinalAssistant {
    /// 结束 `run_agent_turn` 外层的本次循环
    StopTurn,
    /// 追加一条 user 消息并继续请求模型
    RequestPlanRewrite(Message),
    /// 已达 `plan_rewrite_max_attempts` 且规划仍不合格；assistant 已在 `messages` 中，由运行时发 SSE 后结束。
    StopTurnPlanRewriteExhausted { reason: PlanRewriteExhaustedReason },
    /// 静态规则已通过；需异步跑一次极短侧向 LLM 再决定结束或重写（不计入 `plan_rewrite_attempts` 直至判定不一致后追加重写）。
    StopTurnPendingPlanConsistencyLlm {
        plan: plan_artifact::AgentReplyPlanV1,
        tool_digest: Option<String>,
    },
}

/// `workflow_execute` 经反思控制器处理后的结果：要么执行补丁后的参数，要么直接返回跳过结果字符串。
#[derive(Debug)]
pub struct PreparedWorkflowExecute {
    pub patched_args: String,
    pub execute: bool,
    /// 当 `execute == false` 时作为 tool 结果内容
    pub skipped_result: String,
    pub reflection_inject: Option<Value>,
}

/// Web / CLI 共用的 PER 状态。
pub struct PerCoordinator {
    reflection: WorkflowReflectionController,
    final_plan_policy: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    final_plan_require_strict_workflow_node_coverage: bool,
    final_plan_semantic_check_enabled: bool,
    final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 在 [`FinalPlanRequirementMode::WorkflowReflection`] 下，由 `prepare_workflow_execute` 根据反思注入置位。
    plan_requirement_source: PlanRequirementSource,
    plan_rewrite_attempts: usize,
    /// 缓存 [`last_workflow_validate_layer_count`]：`messages.len()` 未变时复用上一次的扫描结果。
    /// [`Self::append_tool_result_and_reflection`] 在追加后按新历史重算；[`Self::invalidate_workflow_validate_layer_cache_after_context_mutation`] 在上下文裁剪/摘要后清空，避免误用旧值。
    cached_workflow_validate_layer_count: Option<usize>,
    layer_count_cache_at_message_len: usize,
}

impl PerCoordinator {
    pub fn plan_rewrite_exhausted_sse_message() -> &'static str {
        PLAN_REWRITE_EXHAUSTED_SSE
    }

    /// 供 `/status` 等只读镜像：`after_final_assistant` 已递增后的重写次数。
    pub fn plan_rewrite_attempts_snapshot(&self) -> usize {
        self.plan_rewrite_attempts
    }

    /// 配置中的规划重写上限（与 `plan_rewrite_max_attempts` 一致）。
    pub fn plan_rewrite_max_attempts_limit(&self) -> usize {
        self.plan_rewrite_max_attempts
    }

    /// 侧向语义校验判定不一致后，递增重写计数（与 `RequestPlanRewrite` 路径一致）。
    pub(crate) fn increment_plan_rewrite_attempts(&mut self) {
        self.plan_rewrite_attempts += 1;
    }

    pub(crate) fn plan_semantic_mismatch_rewrite_message_with_feedback(
        violation_codes: &[String],
        rationale: Option<&str>,
    ) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(
                plan_rewrite_user_text_semantic_mismatch_with_feedback(violation_codes, rationale)
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
            final_plan_require_strict_workflow_node_coverage: init
                .final_plan_require_strict_workflow_node_coverage,
            final_plan_semantic_check_enabled: init.final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: init
                .final_plan_semantic_check_max_non_readonly_tools,
            plan_requirement_source: initial_source,
            plan_rewrite_attempts: 0,
            cached_workflow_validate_layer_count: None,
            layer_count_cache_at_message_len: 0,
        }
    }

    /// `context_window` 在裁剪/摘要等**就地**改写 `messages` 后调用，避免 `layer_count` 缓存指向已删除的 `workflow_validate` 工具结果。
    pub fn invalidate_workflow_validate_layer_cache_after_context_mutation(&mut self) {
        self.cached_workflow_validate_layer_count = None;
        self.layer_count_cache_at_message_len = 0;
    }

    fn workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        let len = messages.len();
        if len != self.layer_count_cache_at_message_len {
            let n = last_workflow_validate_layer_count(messages);
            self.cached_workflow_validate_layer_count = n;
            self.layer_count_cache_at_message_len = len;
            return n;
        }
        if self.cached_workflow_validate_layer_count.is_some() {
            return self.cached_workflow_validate_layer_count;
        }
        let n = last_workflow_validate_layer_count(messages);
        self.cached_workflow_validate_layer_count = n;
        self.layer_count_cache_at_message_len = len;
        n
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
        let require_plan = match self.final_plan_policy {
            FinalPlanRequirementMode::Never => false,
            FinalPlanRequirementMode::WorkflowReflection => {
                self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
            }
            FinalPlanRequirementMode::Always => true,
        };

        log::info!(
            target: "crabmate::per",
            "after_final_assistant enter policy={:?} require_plan={} plan_requirement_source={:?} reflection_stage_round={} plan_rewrite_attempts={} plan_rewrite_max={}",
            self.final_plan_policy,
            require_plan,
            self.plan_requirement_source,
            self.reflection.stage_round(),
            self.plan_rewrite_attempts,
            self.plan_rewrite_max_attempts
        );

        if !require_plan {
            log::info!(
                target: "crabmate::per",
                "after_final_assistant outcome=stop_no_requirement"
            );
            return AfterFinalAssistant::StopTurn;
        }

        let apply_layer_semantics = match self.final_plan_policy {
            FinalPlanRequirementMode::Never => false,
            FinalPlanRequirementMode::WorkflowReflection => {
                self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
            }
            FinalPlanRequirementMode::Always => true,
        };
        let layer_need = self.workflow_validate_layer_need(messages);
        let validate_only_binding_ids = last_workflow_validate_binding_plan_node_ids(messages);

        let content = crate::types::message_content_as_str(&msg.content).unwrap_or("");
        if let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1(content) {
            let layers_ok = match layer_need {
                Some(n) if n > 0 && apply_layer_semantics => plan.steps.len() >= n,
                _ => true,
            };
            let wf_ids = last_workflow_tool_node_ids(messages);
            let workflow_subset_ok = match wf_ids.as_ref() {
                Some(ids) => {
                    plan_artifact::validate_plan_workflow_node_ids_subset(&plan, ids).is_ok()
                }
                None => true,
            };
            let workflow_cover_ok = if self.final_plan_require_strict_workflow_node_coverage {
                match wf_ids.as_ref() {
                    Some(ids) => {
                        plan_artifact::validate_plan_covers_all_workflow_node_ids(&plan, ids)
                            .is_ok()
                    }
                    None => true,
                }
            } else {
                true
            };
            let workflow_ids_ok = workflow_subset_ok && workflow_cover_ok;
            let validate_only_binding_ok = if apply_layer_semantics {
                match validate_only_binding_ids.as_ref() {
                    Some(ids) if !ids.is_empty() => {
                        plan_artifact::validate_plan_binds_workflow_validate_nodes(&plan, ids)
                            .is_ok()
                    }
                    _ => true,
                }
            } else {
                true
            };
            if layers_ok && workflow_ids_ok && validate_only_binding_ok {
                let digest = summarize_messages_for_final_plan_semantic_check(
                    messages,
                    cfg,
                    workspace_is_set,
                    self.final_plan_semantic_check_max_non_readonly_tools,
                );
                let want_llm = self.final_plan_semantic_check_enabled
                    && matches!(
                        self.final_plan_policy,
                        FinalPlanRequirementMode::WorkflowReflection
                    )
                    && self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
                    && digest.is_some();
                if want_llm {
                    log::info!(
                        target: "crabmate::per",
                        "after_final_assistant outcome=pending_plan_consistency_llm plan_steps={} layer_need={:?}",
                        plan.steps.len(),
                        layer_need
                    );
                    return AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm {
                        plan,
                        tool_digest: digest,
                    };
                }
                log::info!(
                    target: "crabmate::per",
                    "after_final_assistant outcome=stop_plan_ok plan_steps={} layer_need={:?}",
                    plan.steps.len(),
                    layer_need
                );
                return AfterFinalAssistant::StopTurn;
            }
            log::info!(
                target: "crabmate::per",
                "after_final_assistant outcome=plan_schema_ok_semantics_fail plan_steps={} layer_need={:?} workflow_node_ids_ok={} validate_only_binding_ok={}",
                plan.steps.len(),
                layer_need,
                workflow_ids_ok,
                validate_only_binding_ok
            );
        }

        if self.plan_rewrite_attempts >= self.plan_rewrite_max_attempts {
            let reason = classify_plan_rewrite_exhausted_reason(
                msg,
                messages,
                layer_need,
                apply_layer_semantics,
                self.final_plan_require_strict_workflow_node_coverage,
            );
            log::warn!(
                target: "crabmate::per",
                "after_final_assistant outcome=plan_rewrite_exhausted layer_need={:?} reason={:?}",
                layer_need,
                reason
            );
            return AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason };
        }
        self.plan_rewrite_attempts += 1;
        let validate_only_bind_ids = validate_only_binding_ids.as_ref().filter(|v| !v.is_empty());
        let bind_suffix = validate_only_bind_ids
            .map(|ids| validate_only_plan_binding_rewrite_suffix(ids.as_slice()))
            .unwrap_or_default();
        let rewrite_text = match (
            layer_need.filter(|&n| n > 0 && apply_layer_semantics),
            last_workflow_tool_node_ids(messages),
        ) {
            (Some(n), Some(ids)) if !ids.is_empty() => {
                let strict = if self.final_plan_require_strict_workflow_node_coverage {
                    format!(
                        "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                        ids.join(", ")
                    )
                } else {
                    String::new()
                };
                format!(
                    "{}\n\n补充：\n- 最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。\n- 若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与 `nodes[].id` 对齐）：{}。{}",
                    plan_rewrite_user_text(),
                    ids.join(", "),
                    strict
                )
            }
            (Some(n), _) => format!(
                "{}\n\n补充：最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。",
                plan_rewrite_user_text()
            ),
            (None, Some(ids)) if !ids.is_empty() => {
                let strict = if self.final_plan_require_strict_workflow_node_coverage {
                    format!(
                        "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                        ids.join(", ")
                    )
                } else {
                    String::new()
                };
                format!(
                    "{}\n\n补充：若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与最近一次 `workflow_execute` 工具结果中 `nodes[].id` 对齐）：{}。{}",
                    plan_rewrite_user_text(),
                    ids.join(", "),
                    strict
                )
            }
            (None, _) => plan_rewrite_user_text(),
        };
        let rewrite_text = format!("{rewrite_text}{bind_suffix}");
        log::info!(
            target: "crabmate::per",
            "after_final_assistant outcome=request_plan_rewrite attempt={} layer_need={:?}",
            self.plan_rewrite_attempts,
            layer_need
        );
        AfterFinalAssistant::RequestPlanRewrite(Message {
            role: "user".to_string(),
            content: Some(rewrite_text.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        })
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
        per_coord.layer_count_cache_at_message_len = messages.len();
        per_coord.cached_workflow_validate_layer_count =
            last_workflow_validate_layer_count(messages.as_slice());
    }
}

#[cfg(test)]
impl PerCoordinator {
    fn test_workflow_validate_layer_need(&mut self, messages: &[Message]) -> Option<usize> {
        self.workflow_validate_layer_need(messages)
    }

    fn test_layer_cache_snapshot(&self) -> (Option<usize>, usize) {
        (
            self.cached_workflow_validate_layer_count,
            self.layer_count_cache_at_message_len,
        )
    }
}

/// 从对话历史中取**最近一次** `workflow_execute` 工具结果中的 `nodes[].id`（`workflow_validate_result` / `workflow_execute_result`）。
fn last_workflow_tool_node_ids(messages: &[Message]) -> Option<Vec<String>> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let name = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)
            .map(|c| c.function.name.as_str())?;
        if name != "workflow_execute" {
            continue;
        }
        let body = crate::types::message_content_as_str(&m.content)?;
        let payload = crate::tool_result::tool_message_payload_for_inner_parse(body);
        let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
        let rt = v.get("report_type").and_then(|x| x.as_str());
        if !matches!(
            rt,
            Some("workflow_validate_result") | Some("workflow_execute_result")
        ) {
            continue;
        }
        let nodes = v.get("nodes").and_then(|x| x.as_array())?;
        let mut ids = Vec::new();
        for n in nodes {
            let id = n.get("id").and_then(|x| x.as_str())?;
            ids.push(id.to_string());
        }
        if ids.is_empty() {
            continue;
        }
        return Some(ids);
    }
    None
}

/// 从对话历史中取**最近一次** `workflow_execute` 的 `workflow_validate_only` 工具结果里的 `spec.layer_count`。
fn last_workflow_validate_layer_count(messages: &[Message]) -> Option<usize> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let name = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)
            .map(|c| c.function.name.as_str())?;
        if name != "workflow_execute" {
            continue;
        }
        let body = crate::types::message_content_as_str(&m.content)?;
        let payload = crate::tool_result::tool_message_payload_for_inner_parse(body);
        let v: Value = serde_json::from_str(payload.as_ref()).ok()?;
        if v.get("report_type").and_then(|x| x.as_str()) != Some("workflow_validate_result") {
            continue;
        }
        let n = v
            .get("spec")
            .and_then(|s| s.get("layer_count"))
            .and_then(|x| x.as_u64())? as usize;
        return Some(n);
    }
    None
}

/// 自历史中取最近一次 **`workflow_validate_result`** 的 `nodes[].id` 列表（含重复），供 **validate_only → Do** 路径上强制规划逐步绑定。
fn last_workflow_validate_binding_plan_node_ids(messages: &[Message]) -> Option<Vec<String>> {
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let Some(tid) = m.tool_call_id.as_deref() else {
            continue;
        };
        let Some(aidx) = assistant_index_for_tool_call(messages, i, tid) else {
            continue;
        };
        let assistant = &messages[aidx];
        let Some(name) = assistant
            .tool_calls
            .as_ref()
            .and_then(|tc| tc.iter().find(|c| c.id == tid))
            .map(|c| c.function.name.as_str())
        else {
            continue;
        };
        if name != "workflow_execute" {
            continue;
        }
        let Some(body) = crate::types::message_content_as_str(&m.content) else {
            continue;
        };
        let payload = crate::tool_result::tool_message_payload_for_inner_parse(body);
        let Ok(v) = serde_json::from_str::<Value>(payload.as_ref()) else {
            continue;
        };
        if v.get("report_type").and_then(|x| x.as_str()) != Some("workflow_validate_result") {
            continue;
        }
        let Some(nodes) = v.get("nodes").and_then(|x| x.as_array()) else {
            continue;
        };
        let mut ids = Vec::new();
        for n in nodes {
            let Some(id) = n.get("id").and_then(|x| x.as_str()) else {
                continue;
            };
            ids.push(id.to_string());
        }
        if ids.is_empty() {
            continue;
        }
        return Some(ids);
    }
    None
}

fn validate_only_plan_binding_rewrite_suffix(validate_only_node_ids: &[String]) -> String {
    if validate_only_node_ids.is_empty() {
        return String::new();
    }
    let n = validate_only_node_ids.len();
    format!(
        "\n\n**validate_only 绑定点（必守）**：最近一次 `workflow_validate_only` 的 `nodes` 共 **{n}** 个（DAG 顺序可异于下列列表，但绑定须一致）。你的 `agent_reply_plan` 须满足：\n\
1. `steps.len()` **等于** **{n}**（与 `nodes` 个数相同）。\n\
2. **每一步**均须设置 **`workflow_node_id`**（不得省略）。\n\
3. 全部 `workflow_node_id` 构成的**多重集合**须与下列节点 id **完全一致**（含重复次数；`steps` 顺序可与下列不同）：`{}`。",
        validate_only_node_ids.join(", ")
    )
}

/// 内置工具名：出现则认为「高风险」，语义侧向校验可收录其摘要（仍受 `max_non_readonly` 条数限制）。
const SEMANTIC_CHECK_HIGH_RISK_TOOLS: &[&str] = &[
    "run_command",
    "run_executable",
    "workflow_execute",
    "create_file",
    "edit_file",
    "apply_patch",
    "http_request",
];

fn tool_name_is_high_risk(name: &str) -> bool {
    SEMANTIC_CHECK_HIGH_RISK_TOOLS.contains(&name) || name.starts_with("mcp__")
}

/// 自尾向前收集最近若干条 `role: tool` 的短摘要，供终答规划侧向 LLM 使用；无工具则 `None`。
fn summarize_messages_for_final_plan_semantic_check(
    messages: &[Message],
    cfg: &AgentConfig,
    workspace_is_set: bool,
    max_non_readonly_tools: usize,
) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut non_ro_used = 0usize;
    const MAX_LINES: usize = 12;
    const MAX_CHARS_PER: usize = 900;

    for i in (0..messages.len()).rev() {
        if lines.len() >= MAX_LINES {
            break;
        }
        let m = &messages[i];
        if m.role != "tool" {
            continue;
        }
        let tid = m.tool_call_id.as_deref()?;
        let aidx = assistant_index_for_tool_call(messages, i, tid)?;
        let assistant = &messages[aidx];
        let tc = assistant
            .tool_calls
            .as_ref()?
            .iter()
            .find(|c| c.id == tid)?;
        let name = tc.function.name.as_str();
        let body = crate::types::message_content_as_str(&m.content).unwrap_or("");

        let is_ro = tool_registry::is_readonly_tool(cfg, name);
        let is_risky = tool_name_is_high_risk(name);
        if !is_ro {
            if non_ro_used >= max_non_readonly_tools && !is_risky {
                continue;
            }
            if !is_ro {
                non_ro_used += 1;
            }
        }

        let mut line = if let Some(env) = crate::tool_result::normalize_tool_message_content(body) {
            let out_head = crate::redact::preview_chars(env.output.as_str(), 320);
            format!(
                "- {} ok={} summary={} out_preview={}",
                env.name,
                env.ok,
                crate::redact::single_line_preview(env.summary.as_str(), 160),
                out_head
            )
        } else {
            format!(
                "- {} legacy_preview={}",
                name,
                crate::redact::preview_chars(body, MAX_CHARS_PER)
            )
        };
        if line.chars().count() > MAX_CHARS_PER {
            line = crate::redact::preview_chars(&line, MAX_CHARS_PER);
        }
        lines.push(line);
    }

    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    let header = if workspace_is_set {
        "以下为逆序收集的最近工具结果摘要（较新在后）；请判断与 agent_reply_plan 是否矛盾。"
    } else {
        "以下为逆序收集的最近工具结果摘要（工作区未设置，可能较不完整）；请判断与 agent_reply_plan 是否矛盾。"
    };
    Some(format!("{}\n{}", header, lines.join("\n")))
}

fn assistant_index_for_tool_call(
    messages: &[Message],
    tool_idx: usize,
    tool_call_id: &str,
) -> Option<usize> {
    for j in (0..tool_idx).rev() {
        if messages[j].role != "assistant" {
            continue;
        }
        let calls = messages[j].tool_calls.as_ref()?;
        if calls.iter().any(|c| c.id == tool_call_id) {
            return Some(j);
        }
    }
    None
}

fn stop_output_to_string(stop_output: Option<Value>) -> String {
    let stop_v = stop_output.unwrap_or_else(|| {
        Value::String("workflow_execute 已停止（反思控制器拒绝继续执行）。".to_string())
    });
    match stop_v {
        Value::String(s) => s,
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, MessageContent, ToolCall};

    fn test_cfg() -> AgentConfig {
        crate::config::load_config(None).expect("embed default config")
    }

    fn pc(policy: FinalPlanRequirementMode, plan_rewrite_max: usize) -> PerCoordinator {
        PerCoordinator::new(PerCoordinatorInit {
            reflection_default_max_rounds: 5,
            final_plan_policy: policy,
            plan_rewrite_max_attempts: plan_rewrite_max,
            final_plan_require_strict_workflow_node_coverage: false,
            final_plan_semantic_check_enabled: false,
            final_plan_semantic_check_max_non_readonly_tools: 0,
        })
    }

    #[test]
    fn final_assistant_rewrites_then_stops() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan here".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist: Vec<Message> = vec![];
        assert!(matches!(
            c.after_final_assistant(&empty, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &hist, &cfg, false),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanMissing
            }
        ));
    }

    #[test]
    fn final_assistant_stops_when_plan_present() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist_one_node = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc0".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"only"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc0".to_string()),
            },
        ];
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"step","workflow_node_id":"only"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok, &hist_one_node, &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn plan_semantics_requires_enough_steps_vs_validate_layers() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let one_step = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"only","description":"only one step here"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&one_step, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let three_steps = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s0","description":"layer 0","workflow_node_id":"a"},
  {"id":"s1","description":"layer 1","workflow_node_id":"b"},
  {"id":"s2","description":"layer 2","workflow_node_id":"c"}
]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&three_steps, &hist, &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn plan_rewrite_exhausted_reason_layer_mismatch() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 1);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let one_step = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"only","description":"only one step here"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&one_step, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&one_step, &hist, &cfg, false),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanLayerCountMismatch
            }
        ));
    }

    #[test]
    fn plan_workflow_node_id_must_match_last_workflow_nodes() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"fmt"},{"id":"test"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let bad_link = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"step1","description":"run fmt","workflow_node_id":"no-such-node"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&bad_link, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let ok_link = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"step1","description":"run fmt","workflow_node_id":"fmt"},
  {"id":"step2","description":"run test","workflow_node_id":"test"}
]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok_link, &hist, &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn validate_only_duplicate_nodes_plan_must_repeat_workflow_node_id() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 3);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"dup"},{"id":"dup"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let one_step_dup = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"both","workflow_node_id":"dup"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&one_step_dup, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let two_steps_dup = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s1","description":"first","workflow_node_id":"dup"},
  {"id":"s2","description":"second","workflow_node_id":"dup"}
]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&two_steps_dup, &hist, &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn strict_workflow_coverage_requires_all_nodes_when_any_workflow_node_id() {
        let cfg = test_cfg();
        let mut c = PerCoordinator::new(PerCoordinatorInit {
            reflection_default_max_rounds: 5,
            final_plan_policy: FinalPlanRequirementMode::WorkflowReflection,
            plan_rewrite_max_attempts: 3,
            final_plan_require_strict_workflow_node_coverage: true,
            final_plan_semantic_check_enabled: false,
            final_plan_semantic_check_max_non_readonly_tools: 0,
        });
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":1},"nodes":[{"id":"fmt"},{"id":"test"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let partial = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"only fmt","workflow_node_id":"fmt"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&partial, &hist, &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        let both = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[
  {"id":"s1","description":"fmt","workflow_node_id":"fmt"},
  {"id":"s2","description":"test","workflow_node_id":"test"}
]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&both, &hist, &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn prepare_workflow_first_round_injects_plan_next() {
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let prep = c.prepare_workflow_execute(args);
        assert!(prep.execute);
        assert!(prep.skipped_result.is_empty());
        let ty = prep
            .reflection_inject
            .as_ref()
            .and_then(|v| v.get("instruction_type"))
            .and_then(|x| x.as_str());
        assert_eq!(
            ty,
            Some(workflow_reflection_controller::INSTRUCTION_WORKFLOW_REFLECTION_PLAN_NEXT)
        );
    }

    #[test]
    fn never_policy_skips_plan_rewrite_even_after_workflow_reflection_inject() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::Never, 2);
        let wf_args = r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan here".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn always_policy_requests_rewrite_without_workflow() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::Always, 2);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
    }

    #[test]
    fn always_policy_stops_when_plan_present() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::Always, 2);
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"Here is my plan:
```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"do the thing"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&ok, &[], &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn always_policy_exhausts_rewrites_then_stops() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::Always, 1);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan at all".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanMissing
            }
        ));
    }

    #[test]
    fn workflow_reflection_no_inject_means_no_requirement() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::StopTurn
        ));
    }

    #[test]
    fn workflow_reflection_done_true_does_not_set_plan_requirement() {
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let wf_args_round1 =
            r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":false}}"#;
        let _ = c.prepare_workflow_execute(wf_args_round1);
        assert!(c.require_plan_in_final_flag_snapshot());

        let wf_args_done =
            r#"{"workflow":{"reflection":{"enabled":true,"max_rounds":2},"done":true}}"#;
        let prep = c.prepare_workflow_execute(wf_args_done);
        assert!(prep.execute);
    }

    #[test]
    fn prepare_workflow_reflection_disabled_passes_through() {
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        let args = r#"{"workflow":{"nodes":[{"id":"a","tool":"ls"}]}}"#;
        let prep = c.prepare_workflow_execute(args);
        assert!(prep.execute);
        assert!(prep.reflection_inject.is_none());
        assert!(!c.require_plan_in_final_flag_snapshot());
    }

    #[test]
    fn plan_rewrite_attempts_increments_correctly() {
        let cfg = test_cfg();
        let mut c = pc(FinalPlanRequirementMode::Always, 3);
        let empty = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 0);
        let _ = c.after_final_assistant(&empty, &[], &cfg, false);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 1);
        let _ = c.after_final_assistant(&empty, &[], &cfg, false);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 2);
        let _ = c.after_final_assistant(&empty, &[], &cfg, false);
        assert_eq!(c.plan_rewrite_attempts_snapshot(), 3);
        assert!(matches!(
            c.after_final_assistant(&empty, &[], &cfg, false),
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanMissing
            }
        ));
    }

    #[test]
    fn append_tool_result_and_reflection_with_inject() {
        let mut msgs: Vec<Message> = vec![];
        let inject = serde_json::json!({
            "instruction_type": "test_instruction",
            "body": "do something"
        });
        let mut c = pc(FinalPlanRequirementMode::Never, 2);
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-99".to_string(),
            "tool output".to_string(),
            Some(inject),
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "tool");
        assert_eq!(
            crate::types::message_content_as_str(&msgs[0].content),
            Some("tool output")
        );
        assert_eq!(msgs[0].tool_call_id.as_deref(), Some("tc-99"));
        assert_eq!(msgs[1].role, "user");
        assert!(
            crate::types::message_content_as_str(&msgs[1].content)
                .unwrap()
                .contains("test_instruction")
        );
    }

    #[test]
    fn workflow_reflection_next_inject_sets_plan_requirement_source() {
        let mut c = pc(FinalPlanRequirementMode::WorkflowReflection, 2);
        assert!(!c.require_plan_in_final_flag_snapshot());
        let mut msgs: Vec<Message> = vec![];
        let inject = serde_json::json!({
            "instruction_type": "workflow_reflection_next",
            "round": 2
        });
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-wf".to_string(),
            "ok".to_string(),
            Some(inject),
        );
        assert!(c.require_plan_in_final_flag_snapshot());
    }

    #[test]
    fn append_tool_result_without_reflection() {
        let mut msgs: Vec<Message> = vec![];
        let mut c = pc(FinalPlanRequirementMode::Never, 2);
        PerCoordinator::append_tool_result_and_reflection(
            &mut c,
            &mut msgs,
            "tc-1".to_string(),
            "result".to_string(),
            None,
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "tool");
    }

    #[test]
    fn layer_count_from_empty_history() {
        assert_eq!(last_workflow_validate_layer_count(&[]), None);
    }

    #[test]
    fn layer_count_from_non_workflow_history() {
        let msgs = vec![
            Message {
                role: "user".to_string(),
                content: Some(MessageContent::Text("hello".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: Some(MessageContent::Text("hi".to_string())),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        assert_eq!(last_workflow_validate_layer_count(&msgs), None);
    }

    fn hist_with_validate_layer_3() -> Vec<Message> {
        vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc1".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "workflow_execute".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"x"},{"id":"y"},{"id":"z"}]}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ]
    }

    #[test]
    fn workflow_validate_layer_cache_reuses_when_len_unchanged() {
        let mut c = pc(FinalPlanRequirementMode::Never, 2);
        let hist = hist_with_validate_layer_3();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
        assert_eq!(c.test_layer_cache_snapshot(), (Some(3), hist.len()));
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
    }

    #[test]
    fn workflow_validate_layer_cache_invalidates_on_context_mutation() {
        let mut c = pc(FinalPlanRequirementMode::Never, 2);
        let mut hist = hist_with_validate_layer_3();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), Some(3));
        hist.pop();
        assert_eq!(c.test_workflow_validate_layer_need(&hist), None);
        assert_eq!(c.test_layer_cache_snapshot(), (None, hist.len()));
    }

    #[test]
    fn workflow_validate_layer_from_crabmate_tool_envelope() {
        let inner = r#"{"report_type":"workflow_validate_result","status":"planned","spec":{"layer_count":3},"nodes":[{"id":"x"},{"id":"y"},{"id":"z"}]}"#;
        let parsed = crate::tool_result::parse_legacy_output("workflow_execute", inner);
        let wrapped = crate::tool_result::encode_tool_message_envelope_v1(
            "workflow_execute",
            "wf".into(),
            &parsed,
            inner,
            None,
        );
        let mut hist = hist_with_validate_layer_3();
        hist[1].content = Some(wrapped.into());
        assert_eq!(last_workflow_validate_layer_count(&hist), Some(3));
    }
}
