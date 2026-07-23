//! 非分层分阶段意图门控的**纯逻辑**（L1 同步评估 + Execute 资格判定；完整 L0+L1+L2 异步路径仍在根包）。

use crabmate_config::AgentConfig;
use crabmate_tools::redact;
use crabmate_types::Message;

use crate::agent_turn::decision_engine::DecisionEngineMode;
use crate::agent_turn::decision_engine::evaluate_intent_only;
use crate::agent_turn::decision_engine::evaluate_scored_with_config;
use crate::agent_turn::decision_engine::scorer::FactorWeights;
use crate::agent_turn::decision_engine::types::FactorContext;
use crate::agent_turn::decision_engine::types::OrchestrationRoute;
use crate::agent_turn::intent::context::build_intent_routing_context;
use crate::agent_turn::intent::user;
use crate::agent_turn::{StagedPlanningDenyReason, StagedPlanningGateOutcome};
use crate::intent_pipeline::{IntentAction, IntentDecision, assess_and_route};
use crate::intent_router::ExecuteIntentThresholds;

fn decision_engine_mode_from_config() -> DecisionEngineMode {
    DecisionEngineMode::Auto
}

fn intent_action_discriminant(action: &IntentAction) -> &'static str {
    match action {
        IntentAction::Execute => "execute",
        IntentAction::DirectReply(_) => "direct_reply",
        IntentAction::ClarifyThenExecute(_) => "clarify_then_execute",
        IntentAction::ConfirmThenExecute(_) => "confirm_then_execute",
    }
}

/// 在 **`IntentAction::Execute`** 且通过简单构建门控时返回 `Ok`；否则返回对应 **Deny** 原因（含非 Execute）。
///
/// 内部委托 `DecisionEngine`（Phase 1 仅含 `IntentFactor`，行为与原有逻辑完全一致）。
pub fn staged_plan_eligibility_for_intent(
    task: &str,
    decision: &IntentDecision,
    mode: DecisionEngineMode,
    threshold: f32,
    weights: &FactorWeights,
) -> Result<(), StagedPlanningDenyReason> {
    let ctx = FactorContext {
        decision,
        task,
        messages: &[],
        cfg: None,
        workspace_file_count: None,
    };
    let result = match mode {
        DecisionEngineMode::Auto => evaluate_intent_only(&ctx),
        DecisionEngineMode::Scored => {
            evaluate_scored_with_config(&ctx, Some(threshold), Some(weights))
        }
    };
    match result.route {
        OrchestrationRoute::Staged => Ok(()),
        OrchestrationRoute::ReAct => Err(StagedPlanningDenyReason::IntentPipelineNotExecute),
    }
}

fn log_staged_gate_outcome(
    task: &str,
    decision: &IntentDecision,
    sse_tag: &str,
    eligibility: Result<(), StagedPlanningDenyReason>,
) {
    let allowed = eligibility.is_ok();
    let deny_reason = eligibility.err().map(StagedPlanningDenyReason::as_str);
    log::info!(
        target: "crabmate",
        "{sse_tag} outcome={} reason={} task_preview={} kind={:?} primary={} action_discriminant={} confidence={:.3}",
        if allowed { "allow" } else { "deny" },
        if allowed {
            "execute_intent"
        } else {
            deny_reason.unwrap_or("deny")
        },
        redact::preview_chars(task, 80),
        decision.kind,
        decision.primary_intent,
        intent_action_discriminant(&decision.action),
        decision.confidence
    );
}

fn gate_outcome_from_decision(
    task: String,
    decision: IntentDecision,
    _cfg: &AgentConfig,
    sse_tag: &str,
) -> StagedPlanningGateOutcome {
    let mode = decision_engine_mode_from_config();
    let threshold = 0.4;
    let weights = FactorWeights::default();
    let eligibility =
        staged_plan_eligibility_for_intent(task.as_str(), &decision, mode, threshold, &weights);
    log_staged_gate_outcome(task.as_str(), &decision, sse_tag, eligibility);
    match eligibility {
        Ok(()) => StagedPlanningGateOutcome::Allow {
            task_preview: task,
            intent_kind: decision.kind,
            primary_intent: decision.primary_intent.clone(),
            confidence: decision.confidence,
            decision,
        },
        Err(reason) => StagedPlanningGateOutcome::Deny {
            reason,
            task_preview: Some(task),
            intent_decision: Some(decision),
        },
    }
}

/// 同步门控（仅 **L1**，无 L2）；用于单测与无需 LLM 的探测。
pub fn assess_staged_planning_gate_l1(
    messages: &[Message],
    cfg: &AgentConfig,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow = user::recently_waiting_execute_confirmation(messages);
    let task = user::extract_effective_user_task(messages, in_clarification_flow);
    if task.trim().is_empty() {
        log::info!(
            target: "crabmate",
            "staged_plan_intent_gate outcome=deny reason=empty_effective_task"
        );
        return StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
    }

    let intent_ctx = build_intent_routing_context(
        messages,
        cfg,
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: cfg.intent_routing.intent_non_hier_execute_low_threshold,
            high: cfg.intent_routing.intent_non_hier_execute_high_threshold,
        },
    );
    let decision = assess_and_route(task.as_str(), &intent_ctx);
    gate_outcome_from_decision(task, decision, cfg, "staged_plan_intent_gate_sync")
}

/// 在已有 **`IntentDecision`** 时评估分阶段资格（供根包完整 L0+L1+L2 管线复用）。
pub fn staged_planning_gate_outcome_from_decision(
    task: String,
    decision: IntentDecision,
    cfg: &AgentConfig,
    sse_log_tag: &str,
) -> StagedPlanningGateOutcome {
    gate_outcome_from_decision(task, decision, cfg, sse_log_tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;

    fn execute_decision() -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: None,
        }
    }

    #[test]
    fn non_execute_action_denies() {
        let mut d = execute_decision();
        d.action = IntentAction::DirectReply("hi".into());
        let err = staged_plan_eligibility_for_intent(
            "task",
            &d,
            DecisionEngineMode::Auto,
            0.4,
            &FactorWeights::default(),
        )
        .unwrap_err();
        assert_eq!(err, StagedPlanningDenyReason::IntentPipelineNotExecute);
    }
}
