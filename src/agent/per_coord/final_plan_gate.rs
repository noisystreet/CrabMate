//! 终答 `agent_reply_plan` v1 门控：**显式状态 / 路由** 表述 `(FinalPlanGatePhase, FinalPlanGateEvent) → 终端路由`
//! （见 `docs/design/per_state_machine_consolidation.md`）。
//! **不**修改 `messages`；侧向语义 LLM 仍由调用方在收到 `StopTurnPendingPlanConsistencyLlm` 后执行。
//!
//! 入口为 [`run_final_plan_gate`]：先根据配置解析相位，再对单次事件做一步转移。
//! 侧向 LLM 完成后经 [`run_final_plan_gate_semantic_completed`]（对应设计稿中的 **`PendingSemanticLlm`** 相位的一步转移）。

use crate::agent::per_plan_semantic_check::PlanSemanticLlmOutcome;
use crate::agent::plan_artifact::{self, AgentReplyPlanV1};
use crate::agent::reflection::plan_rewrite;
use crate::config::AgentConfig;
use crate::types::Message;

use super::final_plan_gate_reason::FinalPlanGateDecisionReason;
use super::{AfterFinalAssistant, FinalPlanRequirementMode, PlanRequirementSource};

// --- Types（终答门控 FSM；见 `docs/design/per_state_machine_consolidation.md`） ---

/// 进入门控瞬间所处「编排相位」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalPlanGatePhase {
    /// 当前策略下不要求终答嵌入结构化规划。
    NoRequirement,
    /// 已确认需要规划：校验本轮 assistant 正文中的 `agent_reply_plan` v1。
    CheckStructuredPlan,
    /// 静态规则已通过，侧向语义一致性 LLM 已调度；**下一步**仅适用 [`run_final_plan_gate_semantic_completed`]（不由 [`run_final_plan_gate`] 与 `FinalAssistantArrived` 组合驱动）。
    PendingSemanticLlm,
}

/// 单次门控处理的事件（当前仅一种：模型给出终答 assistant）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalPlanGateEvent {
    FinalAssistantArrived,
}

/// 本次判定采用的结构性路径（用于日志 / 单测；与 `AfterFinalAssistant` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalPlanGateRoute {
    StopNoRequirement,
    AcceptStructuredPlanOk,
    PendingSemanticConsistencyLlm,
    SemanticsFailedRequestRewrite,
    SemanticsFailedRewriteExhausted,
    /// 侧向语义 LLM 判定规划与工具摘要一致。
    SemanticConsistencyAcceptedStop,
    /// 侧向语义不一致且允许再发起一轮重写 user。
    SemanticMismatchRequestRewrite,
    /// 侧向语义不一致且已达重写上限。
    SemanticMismatchRewriteExhausted,
}

/// [`step_check_structured_plan`] 的完整输出（含可选的重写计数递增）。
pub(crate) struct FinalPlanGateStepOutcome {
    pub route: FinalPlanGateRoute,
    pub decision_reason: FinalPlanGateDecisionReason,
    pub after: AfterFinalAssistant,
    pub next_plan_rewrite_count: Option<usize>,
}

/// 将门控一步输出的 `next_plan_rewrite_count` 写入协调器（静态终答与语义 LLM 路径共用）。
pub(crate) fn apply_plan_rewrite_count_from_gate(
    per: &mut super::PerCoordinator,
    outcome: &FinalPlanGateStepOutcome,
) {
    if let Some(n) = outcome.next_plan_rewrite_count {
        per.counters.plan_rewrite_attempts = n;
    }
}

pub(crate) struct FinalPlanGateArgs<'a> {
    pub msg: &'a Message,
    pub messages: &'a [Message],
    pub cfg: &'a AgentConfig,
    pub workspace_is_set: bool,
    pub final_plan_policy: FinalPlanRequirementMode,
    pub plan_requirement_source: PlanRequirementSource,
    pub final_plan_require_strict_workflow_node_coverage: bool,
    pub final_plan_semantic_check_enabled: bool,
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 已由 `PerCoordinator::workflow_validate_layer_need` 解析；与 `messages` 长度缓存一致。
    pub layer_need: Option<usize>,
    pub validate_only_binding_ids: Option<Vec<String>>,
    pub plan_rewrite_attempts: usize,
    pub plan_rewrite_max_attempts: usize,
}

impl FinalPlanGateArgs<'_> {
    fn apply_layer_semantics(&self) -> bool {
        match self.final_plan_policy {
            FinalPlanRequirementMode::Never => false,
            FinalPlanRequirementMode::WorkflowReflection => {
                self.plan_requirement_source == PlanRequirementSource::WorkflowReflection
            }
            FinalPlanRequirementMode::Always => true,
        }
    }
}

// --- FSM 门面（单事件一步：终答 assistant 到达后的单次判定） ---

/// 由策略与来源解析进入门控时的相位（与 `require_plan` 布尔一致）。
pub(crate) fn resolve_final_plan_gate_phase(
    policy: FinalPlanRequirementMode,
    source: PlanRequirementSource,
) -> FinalPlanGatePhase {
    let require_plan = match policy {
        FinalPlanRequirementMode::Never => false,
        FinalPlanRequirementMode::WorkflowReflection => {
            source == PlanRequirementSource::WorkflowReflection
        }
        FinalPlanRequirementMode::Always => true,
    };
    if require_plan {
        FinalPlanGatePhase::CheckStructuredPlan
    } else {
        FinalPlanGatePhase::NoRequirement
    }
}

/// `(相位, 事件) → 一步结果`。`NoRequirement` 通常由调用方提前返回 `StopTurn`；若误传入此相位，仍返回安全默认以免遗漏分支。
pub(crate) fn run_final_plan_gate(
    phase: FinalPlanGatePhase,
    event: FinalPlanGateEvent,
    args: FinalPlanGateArgs<'_>,
) -> FinalPlanGateStepOutcome {
    match (phase, event) {
        (FinalPlanGatePhase::NoRequirement, FinalPlanGateEvent::FinalAssistantArrived) => {
            tracing::info!(
                target: "crabmate::per",
                outcome = "stop_no_requirement",
                gate_route = ?FinalPlanGateRoute::StopNoRequirement,
                gate_phase = ?phase,
                sub_phase = "reflect",
                "after_final_assistant outcome"
            );
            FinalPlanGateStepOutcome {
                route: FinalPlanGateRoute::StopNoRequirement,
                decision_reason: FinalPlanGateDecisionReason::PolicyNoRequirement,
                after: AfterFinalAssistant::StopTurn,
                next_plan_rewrite_count: None,
            }
        }
        (FinalPlanGatePhase::CheckStructuredPlan, FinalPlanGateEvent::FinalAssistantArrived) => {
            step_check_structured_plan(args)
        }
        (FinalPlanGatePhase::PendingSemanticLlm, FinalPlanGateEvent::FinalAssistantArrived) => {
            tracing::warn!(
                target: "crabmate::per",
                gate_phase = ?phase,
                gate_event = ?event,
                sub_phase = "reflect",
                "final_plan_gate unexpected: PendingSemanticLlm requires run_final_plan_gate_semantic_completed"
            );
            FinalPlanGateStepOutcome {
                route: FinalPlanGateRoute::SemanticConsistencyAcceptedStop,
                decision_reason:
                    FinalPlanGateDecisionReason::UnexpectedPendingSemanticOnFinalArrived,
                after: AfterFinalAssistant::StopTurn,
                next_plan_rewrite_count: None,
            }
        }
    }
}

/// **`PendingSemanticLlm`** 相位：侧向语义 LLM 已完成，映射为终答路由（`reflect_impl` 侧直接消费返回的 `FinalPlanGateStepOutcome`）。
pub(crate) fn run_final_plan_gate_semantic_completed(
    outcome: &PlanSemanticLlmOutcome,
    plan_rewrite_attempts: usize,
    plan_rewrite_max_attempts: usize,
) -> FinalPlanGateStepOutcome {
    use crate::agent::reflection::plan_rewrite::PlanRewriteExhaustedReason;

    tracing::debug!(
        target: "crabmate::per",
        gate_phase = ?FinalPlanGatePhase::PendingSemanticLlm,
        consistent = outcome.consistent,
        plan_rewrite_attempts,
        plan_rewrite_max_attempts,
        sub_phase = "reflect",
        "final_plan_gate semantic_completed step"
    );

    if outcome.consistent {
        tracing::info!(
            target: "crabmate::per",
            outcome = "semantic_consistency_ok",
            gate_route = ?FinalPlanGateRoute::SemanticConsistencyAcceptedStop,
            gate_phase = ?FinalPlanGatePhase::PendingSemanticLlm,
            sub_phase = "reflect",
            "after_final_assistant semantic consistency LLM accepted"
        );
        return FinalPlanGateStepOutcome {
            route: FinalPlanGateRoute::SemanticConsistencyAcceptedStop,
            decision_reason: FinalPlanGateDecisionReason::SemanticConsistencyAccepted,
            after: AfterFinalAssistant::StopTurn,
            next_plan_rewrite_count: None,
        };
    }

    if plan_rewrite_attempts >= plan_rewrite_max_attempts {
        tracing::warn!(
            target: "crabmate::per",
            outcome = "semantic_consistency_exhausted",
            gate_route = ?FinalPlanGateRoute::SemanticMismatchRewriteExhausted,
            gate_phase = ?FinalPlanGatePhase::PendingSemanticLlm,
            sub_phase = "reflect",
            "after_final_assistant semantic inconsistency but plan_rewrite exhausted"
        );
        return FinalPlanGateStepOutcome {
            route: FinalPlanGateRoute::SemanticMismatchRewriteExhausted,
            decision_reason: FinalPlanGateDecisionReason::SemanticInconsistencyRewriteExhausted,
            after: AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent,
            },
            next_plan_rewrite_count: None,
        };
    }

    let next_attempt = plan_rewrite_attempts + 1;
    let rewrite_msg = super::PerCoordinator::plan_semantic_mismatch_rewrite_message_with_feedback(
        outcome.violation_codes.as_slice(),
        outcome.rationale.as_deref(),
    );
    tracing::info!(
        target: "crabmate::per",
        outcome = "semantic_consistency_rewrite",
        gate_route = ?FinalPlanGateRoute::SemanticMismatchRequestRewrite,
        gate_phase = ?FinalPlanGatePhase::PendingSemanticLlm,
        attempt = next_attempt,
        sub_phase = "reflect",
        "after_final_assistant semantic inconsistency requesting rewrite"
    );
    FinalPlanGateStepOutcome {
        route: FinalPlanGateRoute::SemanticMismatchRequestRewrite,
        decision_reason: FinalPlanGateDecisionReason::SemanticInconsistencyRewrite,
        after: AfterFinalAssistant::RequestPlanRewrite(rewrite_msg),
        next_plan_rewrite_count: Some(next_attempt),
    }
}

#[derive(Debug)]
enum StaticSemanticsOutcome {
    PassStopTurn,
    PassPendingSemanticLlm {
        plan: AgentReplyPlanV1,
        tool_digest: Option<String>,
    },
    Fail,
}

fn static_semantics_layers_ok(
    plan: &AgentReplyPlanV1,
    apply_layer_semantics: bool,
    layer_need: Option<usize>,
) -> bool {
    match layer_need {
        Some(n) if n > 0 && apply_layer_semantics => plan.steps.len() >= n,
        _ => true,
    }
}

fn static_semantics_workflow_ids_ok(
    plan: &AgentReplyPlanV1,
    args: &FinalPlanGateArgs<'_>,
    wf_ids: &Option<Vec<String>>,
) -> bool {
    let workflow_subset_ok = match wf_ids.as_ref() {
        Some(ids) => plan_artifact::validate_plan_workflow_node_ids_subset(plan, ids).is_ok(),
        None => true,
    };
    let workflow_cover_ok = if args.final_plan_require_strict_workflow_node_coverage {
        match wf_ids.as_ref() {
            Some(ids) => {
                plan_artifact::validate_plan_covers_all_workflow_node_ids(plan, ids).is_ok()
            }
            None => true,
        }
    } else {
        true
    };
    workflow_subset_ok && workflow_cover_ok
}

fn static_semantics_validate_only_binding_ok(
    plan: &AgentReplyPlanV1,
    apply_layer_semantics: bool,
    validate_only_binding_ids: Option<&Vec<String>>,
) -> bool {
    if apply_layer_semantics {
        match validate_only_binding_ids {
            Some(ids) if !ids.is_empty() => {
                plan_artifact::validate_plan_binds_workflow_validate_nodes(plan, ids).is_ok()
            }
            _ => true,
        }
    } else {
        true
    }
}

fn evaluate_static_semantics(
    plan: &AgentReplyPlanV1,
    args: &FinalPlanGateArgs<'_>,
    apply_layer_semantics: bool,
    layer_need: Option<usize>,
    validate_only_binding_ids: Option<&Vec<String>>,
) -> StaticSemanticsOutcome {
    let layers_ok = static_semantics_layers_ok(plan, apply_layer_semantics, layer_need);
    let wf_ids = plan_rewrite::last_workflow_tool_node_ids(args.messages);
    let workflow_ids_ok = static_semantics_workflow_ids_ok(plan, args, &wf_ids);
    let validate_only_binding_ok = static_semantics_validate_only_binding_ok(
        plan,
        apply_layer_semantics,
        validate_only_binding_ids,
    );
    if !(layers_ok && workflow_ids_ok && validate_only_binding_ok) {
        tracing::info!(
            target: "crabmate::per",
            outcome = "plan_schema_ok_semantics_fail",
            plan_steps = plan.steps.len(),
            layer_need = ?layer_need,
            workflow_node_ids_ok = workflow_ids_ok,
            validate_only_binding_ok = validate_only_binding_ok,
            sub_phase = "reflect",
            "after_final_assistant static semantics failed"
        );
        return StaticSemanticsOutcome::Fail;
    }

    let digest = plan_rewrite::summarize_messages_for_final_plan_semantic_check(
        args.messages,
        args.cfg,
        args.workspace_is_set,
        args.final_plan_semantic_check_max_non_readonly_tools,
    );
    let want_llm = args.final_plan_semantic_check_enabled
        && matches!(
            args.final_plan_policy,
            FinalPlanRequirementMode::WorkflowReflection
        )
        && args.plan_requirement_source == PlanRequirementSource::WorkflowReflection
        && digest.is_some();
    if want_llm {
        tracing::info!(
            target: "crabmate::per",
            outcome = "pending_plan_consistency_llm",
            plan_steps = plan.steps.len(),
            layer_need = ?layer_need,
            sub_phase = "reflect",
            "after_final_assistant pending semantic consistency LLM"
        );
        StaticSemanticsOutcome::PassPendingSemanticLlm {
            plan: plan.clone(),
            tool_digest: digest,
        }
    } else {
        tracing::info!(
            target: "crabmate::per",
            outcome = "stop_plan_ok",
            plan_steps = plan.steps.len(),
            layer_need = ?layer_need,
            sub_phase = "reflect",
            "after_final_assistant plan ok stop turn"
        );
        StaticSemanticsOutcome::PassStopTurn
    }
}

fn outcome_after_semantics_failure(args: FinalPlanGateArgs<'_>) -> FinalPlanGateStepOutcome {
    let apply_layer_semantics = args.apply_layer_semantics();
    let layer_need = args.layer_need;
    let validate_only_binding_ids = &args.validate_only_binding_ids;

    if args.plan_rewrite_attempts >= args.plan_rewrite_max_attempts {
        let reason = plan_rewrite::classify_exhausted_reason(
            args.msg,
            args.messages,
            layer_need,
            apply_layer_semantics,
            args.final_plan_require_strict_workflow_node_coverage,
        );
        tracing::warn!(
            target: "crabmate::per",
            outcome = "plan_rewrite_exhausted",
            layer_need = ?layer_need,
            reason = ?reason,
            sub_phase = "reflect",
            "after_final_assistant plan rewrite exhausted"
        );
        return FinalPlanGateStepOutcome {
            route: FinalPlanGateRoute::SemanticsFailedRewriteExhausted,
            decision_reason: FinalPlanGateDecisionReason::PlanRewriteExhausted,
            after: AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason },
            next_plan_rewrite_count: None,
        };
    }
    let next_attempt = args.plan_rewrite_attempts + 1;
    let validate_only_bind_ids = validate_only_binding_ids.as_ref().filter(|v| !v.is_empty());
    let bind_suffix = validate_only_bind_ids
        .map(|ids| plan_rewrite::validate_only_plan_binding_rewrite_suffix(ids.as_slice()))
        .unwrap_or_default();
    let rewrite_text = match (
        layer_need.filter(|&n| n > 0 && apply_layer_semantics),
        plan_rewrite::last_workflow_tool_node_ids(args.messages),
    ) {
        (Some(n), Some(ids)) if !ids.is_empty() => {
            let strict = if args.final_plan_require_strict_workflow_node_coverage {
                format!(
                    "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                    ids.join(", ")
                )
            } else {
                String::new()
            };
            format!(
                "{}\n\n补充：\n- 最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。\n- 若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与 `nodes[].id` 对齐）：{}。{}",
                plan_rewrite::plan_rewrite_user_text_base(),
                ids.join(", "),
                strict
            )
        }
        (Some(n), _) => format!(
            "{}\n\n补充：最近一次 `workflow_validate_only` 结果为 **{n}** 个执行层（`spec.layer_count`）。你的 `agent_reply_plan.steps` 条数须 **不少于 {n}**，且每条 `description` 应能对应到具体层或节点意图。",
            plan_rewrite::plan_rewrite_user_text_base()
        ),
        (None, Some(ids)) if !ids.is_empty() => {
            let strict = if args.final_plan_require_strict_workflow_node_coverage {
                format!(
                    "\n- 若**任一步**填写了 `workflow_node_id`，则须覆盖下列**全部**节点 id（每 id 至少一步）：{}。",
                    ids.join(", ")
                )
            } else {
                String::new()
            };
            format!(
                "{}\n\n补充：若步骤中填写了 `workflow_node_id`，其值须为下列 **workflow 节点 id** 之一的子集（与最近一次 `workflow_execute` 工具结果中 `nodes[].id` 对齐）：{}。{}",
                plan_rewrite::plan_rewrite_user_text_base(),
                ids.join(", "),
                strict
            )
        }
        (None, _) => plan_rewrite::plan_rewrite_user_text_base(),
    };
    let rewrite_text = format!("{rewrite_text}{bind_suffix}");
    tracing::info!(
        target: "crabmate::per",
        outcome = "request_plan_rewrite",
        attempt = next_attempt,
        layer_need = ?layer_need,
        sub_phase = "reflect",
        "after_final_assistant request plan rewrite"
    );
    FinalPlanGateStepOutcome {
        route: FinalPlanGateRoute::SemanticsFailedRequestRewrite,
        decision_reason: FinalPlanGateDecisionReason::StaticSemanticsFailed,
        after: AfterFinalAssistant::RequestPlanRewrite(Message::user_plan_rewrite_injection(
            rewrite_text,
        )),
        next_plan_rewrite_count: Some(next_attempt),
    }
}

/// 在 **`CheckStructuredPlan`** 相位处理 **`FinalAssistantArrived`**（解析 JSON + 静态语义 + 重写 / 挂起侧向 LLM）。
/// 调用方须在返回 `RequestPlanRewrite` 后自行将 `plan_rewrite_attempts` 与之一致（本函数**不**修改计数：先判断耗尽再 `+=1`）。
pub(crate) fn step_check_structured_plan(args: FinalPlanGateArgs<'_>) -> FinalPlanGateStepOutcome {
    tracing::debug!(
        target: "crabmate::per",
        gate_phase = ?FinalPlanGatePhase::CheckStructuredPlan,
        gate_event = ?FinalPlanGateEvent::FinalAssistantArrived,
        sub_phase = "reflect",
        "final_plan_gate step"
    );

    let apply_layer_semantics = args.apply_layer_semantics();
    let layer_need = args.layer_need;
    let validate_only_binding_ids = args.validate_only_binding_ids.as_ref();

    let content = crate::types::message_content_as_str(&args.msg.content).unwrap_or("");
    if let Ok(plan) = plan_artifact::parse_agent_reply_plan_v1_with_validate_only_binding_ids(
        content,
        validate_only_binding_ids.map(|v| v.as_slice()),
    ) {
        match evaluate_static_semantics(
            &plan,
            &args,
            apply_layer_semantics,
            layer_need,
            args.validate_only_binding_ids.as_ref(),
        ) {
            StaticSemanticsOutcome::PassStopTurn => FinalPlanGateStepOutcome {
                route: FinalPlanGateRoute::AcceptStructuredPlanOk,
                decision_reason: FinalPlanGateDecisionReason::StructuredPlanAccepted,
                after: AfterFinalAssistant::StopTurn,
                next_plan_rewrite_count: None,
            },
            StaticSemanticsOutcome::PassPendingSemanticLlm { plan, tool_digest } => {
                FinalPlanGateStepOutcome {
                    route: FinalPlanGateRoute::PendingSemanticConsistencyLlm,
                    decision_reason: FinalPlanGateDecisionReason::PendingSemanticConsistencyLlm,
                    after: AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm {
                        plan,
                        tool_digest,
                    },
                    next_plan_rewrite_count: None,
                }
            }
            StaticSemanticsOutcome::Fail => outcome_after_semantics_failure(args),
        }
    } else {
        let mut outcome = outcome_after_semantics_failure(args);
        if matches!(
            outcome.route,
            FinalPlanGateRoute::SemanticsFailedRequestRewrite
        ) {
            outcome.decision_reason = FinalPlanGateDecisionReason::PlanParseFailed;
        }
        outcome
    }
}

/// 对一次终答 assistant 运行完整门控（**始终**经 [`run_final_plan_gate`]；`NoRequirement` 相位下 `layer_need` 等字段不读取）。
pub(crate) fn after_final_assistant(
    per: &mut super::PerCoordinator,
    msg: &Message,
    messages: &[Message],
    cfg: &AgentConfig,
    workspace_is_set: bool,
) -> AfterFinalAssistant {
    let phase = resolve_final_plan_gate_phase(per.final_plan_policy, per.plan_requirement_source);
    let require_plan = matches!(phase, FinalPlanGatePhase::CheckStructuredPlan);
    let reflection_stage_round = per.reflection.stage_round();

    tracing::info!(
        target: "crabmate::per",
        final_plan_policy = ?per.final_plan_policy,
        require_plan = require_plan,
        plan_requirement_source = ?per.plan_requirement_source,
        reflection_stage_round = reflection_stage_round,
        plan_rewrite_attempts = per.counters.plan_rewrite_attempts,
        plan_rewrite_max = per.plan_rewrite_max_attempts,
        gate_phase = ?phase,
        sub_phase = "reflect",
        "after_final_assistant enter"
    );

    let (layer_need, validate_only_binding_ids) = if require_plan {
        (
            per.workflow_validate_layer_need(messages),
            plan_rewrite::last_workflow_validate_binding_plan_node_ids(messages),
        )
    } else {
        (None, None)
    };

    let outcome = run_final_plan_gate(
        phase,
        FinalPlanGateEvent::FinalAssistantArrived,
        FinalPlanGateArgs {
            msg,
            messages,
            cfg,
            workspace_is_set,
            final_plan_policy: per.final_plan_policy,
            plan_requirement_source: per.plan_requirement_source,
            final_plan_require_strict_workflow_node_coverage: per
                .final_plan_require_strict_workflow_node_coverage,
            final_plan_semantic_check_enabled: per.final_plan_semantic_check_enabled,
            final_plan_semantic_check_max_non_readonly_tools: per
                .final_plan_semantic_check_max_non_readonly_tools,
            layer_need,
            validate_only_binding_ids,
            plan_rewrite_attempts: per.counters.plan_rewrite_attempts,
            plan_rewrite_max_attempts: per.plan_rewrite_max_attempts,
        },
    );
    tracing::debug!(
        target: "crabmate::per",
        gate_route = ?outcome.route,
        gate_decision_reason = outcome.decision_reason.as_str(),
        gate_phase = ?phase,
        sub_phase = "reflect",
        "final_plan_gate transition"
    );
    apply_plan_rewrite_count_from_gate(per, &outcome);
    outcome.after
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::per_plan_semantic_check::PlanSemanticLlmOutcome;
    use crate::agent::reflection::plan_rewrite::PlanRewriteExhaustedReason;
    use crate::types::{FunctionCall, MessageContent, ToolCall};

    fn minimal_cfg() -> AgentConfig {
        crate::config::load_config(None).expect("embed default config")
    }

    fn gate_args<'a>(
        msg: &'a Message,
        messages: &'a [Message],
        cfg: &'a AgentConfig,
        policy: FinalPlanRequirementMode,
        source: PlanRequirementSource,
        attempts: usize,
        max_attempts: usize,
    ) -> FinalPlanGateArgs<'a> {
        FinalPlanGateArgs {
            msg,
            messages,
            cfg,
            workspace_is_set: false,
            final_plan_policy: policy,
            plan_requirement_source: source,
            final_plan_require_strict_workflow_node_coverage: false,
            final_plan_semantic_check_enabled: false,
            final_plan_semantic_check_max_non_readonly_tools: 0,
            layer_need: None,
            validate_only_binding_ids: None,
            plan_rewrite_attempts: attempts,
            plan_rewrite_max_attempts: max_attempts,
        }
    }

    #[test]
    fn gate_route_accept_ok_when_plan_valid() {
        let cfg = minimal_cfg();
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"x"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist: Vec<Message> = vec![];
        let o = step_check_structured_plan(gate_args(
            &ok,
            &hist,
            &cfg,
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::WorkflowReflection,
            0,
            2,
        ));
        assert_eq!(o.route, FinalPlanGateRoute::AcceptStructuredPlanOk);
        assert_eq!(
            o.decision_reason,
            FinalPlanGateDecisionReason::StructuredPlanAccepted
        );
        assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
    }

    #[test]
    fn gate_route_rewrite_when_parse_fails() {
        let cfg = minimal_cfg();
        let bad = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no json plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist: Vec<Message> = vec![];
        let o = step_check_structured_plan(gate_args(
            &bad,
            &hist,
            &cfg,
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::WorkflowReflection,
            0,
            2,
        ));
        assert_eq!(o.route, FinalPlanGateRoute::SemanticsFailedRequestRewrite);
        assert_eq!(
            o.decision_reason,
            FinalPlanGateDecisionReason::PlanParseFailed
        );
        assert!(matches!(
            o.after,
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert_eq!(o.next_plan_rewrite_count, Some(1));
    }

    #[test]
    fn gate_route_exhausted_when_attempts_maxed() {
        let cfg = minimal_cfg();
        let bad = Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no json plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist: Vec<Message> = vec![];
        let o = step_check_structured_plan(gate_args(
            &bad,
            &hist,
            &cfg,
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::WorkflowReflection,
            2,
            2,
        ));
        assert_eq!(o.route, FinalPlanGateRoute::SemanticsFailedRewriteExhausted);
        assert!(matches!(
            o.after,
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanMissing
            }
        ));
    }

    #[test]
    fn resolve_phase_never_is_no_requirement() {
        assert_eq!(
            resolve_final_plan_gate_phase(
                FinalPlanRequirementMode::Never,
                PlanRequirementSource::WorkflowReflection
            ),
            FinalPlanGatePhase::NoRequirement
        );
    }

    #[test]
    fn run_gate_no_requirement_returns_stop_turn() {
        let cfg = minimal_cfg();
        let msg = Message {
            role: "assistant".to_string(),
            content: Some("x".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let o = run_final_plan_gate(
            FinalPlanGatePhase::NoRequirement,
            FinalPlanGateEvent::FinalAssistantArrived,
            gate_args(
                &msg,
                &[],
                &cfg,
                FinalPlanRequirementMode::Never,
                PlanRequirementSource::None,
                0,
                2,
            ),
        );
        assert_eq!(o.route, FinalPlanGateRoute::StopNoRequirement);
        assert_eq!(
            o.decision_reason,
            FinalPlanGateDecisionReason::PolicyNoRequirement
        );
        assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
    }

    #[test]
    fn gate_route_pending_semantic_when_digest_present() {
        let cfg = minimal_cfg();
        let ok = Message {
            role: "assistant".to_string(),
            content: Some(
                r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"x"}]}
```"#
                    .into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let hist = vec![
            Message {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc0".to_string(),
                    typ: "function".to_string(),
                    function: FunctionCall {
                        name: "read_file".to_string(),
                        arguments: r#"{"path":"a.rs"}"#.to_string(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: Some("file contents".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc0".to_string()),
            },
        ];
        let o = step_check_structured_plan(FinalPlanGateArgs {
            msg: &ok,
            messages: &hist,
            cfg: &cfg,
            workspace_is_set: false,
            final_plan_policy: FinalPlanRequirementMode::WorkflowReflection,
            plan_requirement_source: PlanRequirementSource::WorkflowReflection,
            final_plan_require_strict_workflow_node_coverage: false,
            final_plan_semantic_check_enabled: true,
            final_plan_semantic_check_max_non_readonly_tools: 4,
            layer_need: None,
            validate_only_binding_ids: None,
            plan_rewrite_attempts: 0,
            plan_rewrite_max_attempts: 2,
        });
        assert_eq!(o.route, FinalPlanGateRoute::PendingSemanticConsistencyLlm);
        assert_eq!(
            o.decision_reason,
            FinalPlanGateDecisionReason::PendingSemanticConsistencyLlm
        );
        assert!(matches!(
            o.after,
            AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { .. }
        ));
    }

    #[test]
    fn semantic_completed_consistent_stops() {
        let o = run_final_plan_gate_semantic_completed(
            &PlanSemanticLlmOutcome {
                consistent: true,
                violation_codes: vec![],
                rationale: None,
                user_cancelled: false,
            },
            0,
            3,
        );
        assert_eq!(o.route, FinalPlanGateRoute::SemanticConsistencyAcceptedStop);
        assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
        assert_eq!(o.next_plan_rewrite_count, None);
    }

    #[test]
    fn semantic_completed_inconsistent_rewrites() {
        let o = run_final_plan_gate_semantic_completed(
            &PlanSemanticLlmOutcome {
                consistent: false,
                violation_codes: vec!["x".into()],
                rationale: Some("r".into()),
                user_cancelled: false,
            },
            1,
            3,
        );
        assert_eq!(o.route, FinalPlanGateRoute::SemanticMismatchRequestRewrite);
        assert!(matches!(
            o.after,
            AfterFinalAssistant::RequestPlanRewrite(_)
        ));
        assert_eq!(o.next_plan_rewrite_count, Some(2));
    }

    #[test]
    fn semantic_completed_inconsistent_exhausted() {
        let o = run_final_plan_gate_semantic_completed(
            &PlanSemanticLlmOutcome {
                consistent: false,
                violation_codes: vec!["x".into()],
                rationale: None,
                user_cancelled: false,
            },
            3,
            3,
        );
        assert_eq!(
            o.route,
            FinalPlanGateRoute::SemanticMismatchRewriteExhausted
        );
        assert!(matches!(
            o.after,
            AfterFinalAssistant::StopTurnPlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent
            }
        ));
    }
}
