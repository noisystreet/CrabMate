//! 单轮编排路由决议快照（v1 JSON）：门控链结束后一次性记录，供 tracing / SSE / 金样回归。

use crabmate_config::{AgentConfig, FinalPlanRequirementMode};

use crate::intent_pipeline::{IntentAction, IntentDecision};
use crate::intent_router::IntentKind;

use super::hierarchical_intent_route::HierarchicalPostIntentRoute;
use super::orchestration_entry::TurnTopLevelDispatch;
use super::staged_planning_gate_types::StagedPlanningGateOutcome;
use super::turn_orchestration::{NonHierarchicalTurnResolution, TurnOrchestrationMode};

/// 路由决议 JSON 根（`version` 固定为 1）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TurnRouteDecisionV1 {
    pub version: u8,
    pub top_level: String,
    pub intent_gate: IntentGateSnapshot,
    pub staged_gate: StagedGateSnapshot,
    pub turn_phase: String,
    pub orchestration_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeform_because: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hierarchical_post_intent_route: Option<String>,
    pub planner_executor_mode: String,
    pub plan_requirement_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_profile: Option<String>,
}

/// 非分层 **`intent_at_turn_start`** 快照（分层路径在门控后单独填充）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IntentGateSnapshot {
    Disabled,
    EmptyTask,
    FinishedEarly {
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        primary_intent: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<String>,
    },
    ProceedExecute {
        kind: String,
        primary_intent: String,
        action: String,
        confidence: f32,
        need_clarification: bool,
    },
}

/// 非分层 **`staged_plan_intent_gate`** 快照；分层顶层为 [`StagedGateSnapshot::NotEvaluated`]。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StagedGateSnapshot {
    NotEvaluated,
    Allow {
        primary_intent: String,
        confidence: f32,
    },
    Deny {
        reason: String,
    },
}

impl TurnRouteDecisionV1 {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[inline]
pub fn intent_kind_label(kind: IntentKind) -> &'static str {
    match kind {
        IntentKind::Greeting => "greeting",
        IntentKind::Execute => "execute",
        IntentKind::Qa => "qa",
        IntentKind::Ambiguous => "ambiguous",
    }
}

#[inline]
pub fn intent_action_label(action: &IntentAction) -> &'static str {
    match action {
        IntentAction::Execute => "execute",
        IntentAction::ConfirmThenExecute(_) => "confirm_then_execute",
        IntentAction::ClarifyThenExecute(_) => "clarify_then_execute",
        IntentAction::DirectReply(_) => "direct_reply",
    }
}

/// 由 L2 / 意图管线产出构造 **`ProceedExecute`** 快照。
pub fn intent_gate_snapshot_from_decision(decision: &IntentDecision) -> IntentGateSnapshot {
    IntentGateSnapshot::ProceedExecute {
        kind: intent_kind_label(decision.kind).to_string(),
        primary_intent: decision.primary_intent.clone(),
        action: intent_action_label(&decision.action).to_string(),
        confidence: decision.confidence,
        need_clarification: decision.need_clarification,
    }
}

pub fn intent_gate_snapshot_finished_early(decision: &IntentDecision) -> IntentGateSnapshot {
    IntentGateSnapshot::FinishedEarly {
        kind: Some(intent_kind_label(decision.kind).to_string()),
        primary_intent: Some(decision.primary_intent.clone()),
        action: Some(intent_action_label(&decision.action).to_string()),
    }
}

pub fn staged_gate_snapshot_from_outcome(gate: &StagedPlanningGateOutcome) -> StagedGateSnapshot {
    match gate {
        StagedPlanningGateOutcome::Allow {
            primary_intent,
            confidence,
            ..
        } => StagedGateSnapshot::Allow {
            primary_intent: primary_intent.clone(),
            confidence: *confidence,
        },
        StagedPlanningGateOutcome::Deny { reason, .. } => StagedGateSnapshot::Deny {
            reason: reason.as_str().to_string(),
        },
    }
}

fn plan_requirement_policy_label(cfg: &AgentConfig) -> String {
    match cfg.per_plan_policy.final_plan_requirement {
        FinalPlanRequirementMode::Never => "never".to_string(),
        FinalPlanRequirementMode::WorkflowReflection => "workflow_reflection".to_string(),
        FinalPlanRequirementMode::Always => "always".to_string(),
    }
}

fn top_level_label(top: TurnTopLevelDispatch) -> String {
    top.as_str().to_string()
}

/// 非分层：门控链结束且已解析 [`NonHierarchicalTurnResolution`] 后组装决议。
pub fn build_non_hierarchical_turn_route_decision(
    cfg: &AgentConfig,
    intent_gate: IntentGateSnapshot,
    staged_gate: StagedGateSnapshot,
    entry: &NonHierarchicalTurnResolution,
) -> TurnRouteDecisionV1 {
    TurnRouteDecisionV1 {
        version: 1,
        top_level: top_level_label(TurnTopLevelDispatch::NonHierarchical),
        intent_gate,
        staged_gate,
        turn_phase: entry.turn_phase.as_str().to_string(),
        orchestration_mode: entry.orchestration_mode.as_str().to_string(),
        freeform_because: entry.freeform_because.map(|b| b.as_str().to_string()),
        hierarchical_post_intent_route: None,
        planner_executor_mode: cfg
            .per_plan_policy
            .planner_executor_mode
            .as_str()
            .to_string(),
        plan_requirement_policy: plan_requirement_policy_label(cfg),
        orchestration_profile: None,
    }
}

/// 分层：**`intent_at_turn_start`** 已写入终答并结束本回合。
pub fn build_hierarchical_intent_finished_early_decision(
    cfg: &AgentConfig,
    intent_gate: IntentGateSnapshot,
) -> TurnRouteDecisionV1 {
    TurnRouteDecisionV1 {
        version: 1,
        top_level: top_level_label(TurnTopLevelDispatch::Hierarchical),
        intent_gate,
        staged_gate: StagedGateSnapshot::NotEvaluated,
        turn_phase: "intent_gate_finished_early".to_string(),
        orchestration_mode: TurnOrchestrationMode::IntentAtTurnStartFinished
            .as_str()
            .to_string(),
        freeform_because: None,
        hierarchical_post_intent_route: None,
        planner_executor_mode: cfg
            .per_plan_policy
            .planner_executor_mode
            .as_str()
            .to_string(),
        plan_requirement_policy: plan_requirement_policy_label(cfg),
        orchestration_profile: None,
    }
}

/// 非分层：**`intent_at_turn_start`** 已写入终答并结束本回合（未评估 staged 门控）。
pub fn build_non_hierarchical_intent_finished_early_decision(
    cfg: &AgentConfig,
    intent_gate: IntentGateSnapshot,
) -> TurnRouteDecisionV1 {
    TurnRouteDecisionV1 {
        version: 1,
        top_level: top_level_label(TurnTopLevelDispatch::NonHierarchical),
        intent_gate,
        staged_gate: StagedGateSnapshot::NotEvaluated,
        turn_phase: "intent_at_turn_start_finished".to_string(),
        orchestration_mode: TurnOrchestrationMode::IntentAtTurnStartFinished
            .as_str()
            .to_string(),
        freeform_because: None,
        hierarchical_post_intent_route: None,
        planner_executor_mode: cfg
            .per_plan_policy
            .planner_executor_mode
            .as_str()
            .to_string(),
        plan_requirement_policy: plan_requirement_policy_label(cfg),
        orchestration_profile: None,
    }
}

/// 分层：意图门控 **`ProceedExecute`** 且已解析 [`super::orchestration_entry::HierarchicalTurnEntryResolution`]。
pub fn build_hierarchical_turn_route_decision(
    cfg: &AgentConfig,
    intent_gate: IntentGateSnapshot,
    orchestration_mode: TurnOrchestrationMode,
    post_intent_route: HierarchicalPostIntentRoute,
) -> TurnRouteDecisionV1 {
    TurnRouteDecisionV1 {
        version: 1,
        top_level: top_level_label(TurnTopLevelDispatch::Hierarchical),
        intent_gate,
        staged_gate: StagedGateSnapshot::NotEvaluated,
        turn_phase: orchestration_mode.as_str().to_string(),
        orchestration_mode: orchestration_mode.as_str().to_string(),
        freeform_because: None,
        hierarchical_post_intent_route: Some(post_intent_route.as_str().to_string()),
        planner_executor_mode: cfg
            .per_plan_policy
            .planner_executor_mode
            .as_str()
            .to_string(),
        plan_requirement_policy: plan_requirement_policy_label(cfg),
        orchestration_profile: None,
    }
}

/// 结构化 tracing（与 `log_orchestration_transition` 字段对齐）。
pub fn log_turn_route_decision(decision: &TurnRouteDecisionV1) {
    log::info!(
        target: "crabmate::agent_turn",
        "turn_route_decision version={} top_level={} orchestration_mode={} turn_phase={} freeform_because={} hierarchical_post_intent_route={} planner_executor_mode={} plan_requirement_policy={}",
        decision.version,
        decision.top_level,
        decision.orchestration_mode,
        decision.turn_phase,
        decision.freeform_because.as_deref().unwrap_or(""),
        decision.hierarchical_post_intent_route.as_deref().unwrap_or(""),
        decision.planner_executor_mode,
        decision.plan_requirement_policy,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crabmate_config::PlannerExecutorMode;

    fn cfg_with(mode: PlannerExecutorMode) -> AgentConfig {
        let mut c = crabmate_config::load_config(None).expect("embed default config");
        c.per_plan_policy.planner_executor_mode = mode;
        c
    }

    fn execute_gate_allow() -> StagedPlanningGateOutcome {
        StagedPlanningGateOutcome::Allow {
            task_preview: "build".into(),
            intent_kind: IntentKind::Execute,
            primary_intent: "execute.run_test_build".into(),
            confidence: 0.95,
            decision: IntentDecision {
                kind: IntentKind::Execute,
                primary_intent: "execute.run_test_build".into(),
                secondary_intents: Vec::new(),
                confidence: 0.95,
                abstain: false,
                need_clarification: false,
                action: IntentAction::Execute,
            },
        }
    }

    #[test]
    fn build_non_hierarchical_planned_step_json_shape() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = execute_gate_allow();
        let entry = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        let intent = intent_gate_snapshot_from_decision(match &gate {
            StagedPlanningGateOutcome::Allow { decision, .. } => decision,
            _ => panic!("allow"),
        });
        let decision = build_non_hierarchical_turn_route_decision(
            &cfg,
            intent,
            staged_gate_snapshot_from_outcome(&gate),
            &entry,
        );
        assert_eq!(decision.version, 1);
        assert_eq!(decision.orchestration_mode, "planned_step_single_agent");
        assert_eq!(decision.turn_phase, "planned_step_single_agent");
        assert!(decision.freeform_because.is_none());
        let json = decision.to_json().expect("json");
        assert!(json.contains("\"version\":1"));
        assert!(json.contains("\"top_level\":\"non_hierarchical\""));
    }

    #[test]
    fn build_non_hierarchical_freeform_on_deny() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::AdvisoryExecuteBypassStaged,
            task_preview: Some("t".into()),
            intent_decision: None,
        };
        let entry = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        let decision = build_non_hierarchical_turn_route_decision(
            &cfg,
            IntentGateSnapshot::ProceedExecute {
                kind: "execute".into(),
                primary_intent: "execute.code_change".into(),
                action: "execute".into(),
                confidence: 0.9,
                need_clarification: false,
            },
            staged_gate_snapshot_from_outcome(&gate),
            &entry,
        );
        assert_eq!(decision.orchestration_mode, "freeform");
        assert_eq!(
            decision.freeform_because.as_deref(),
            Some("advisory_execute_bypass_staged")
        );
    }

    #[test]
    fn intent_finished_early_decision_mode() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let decision = build_non_hierarchical_intent_finished_early_decision(
            &cfg,
            IntentGateSnapshot::FinishedEarly {
                kind: Some("greeting".into()),
                primary_intent: Some("meta.greeting".into()),
                action: Some("direct_reply".into()),
            },
        );
        assert_eq!(decision.orchestration_mode, "intent_at_turn_start_finished");
        assert!(matches!(
            decision.staged_gate,
            StagedGateSnapshot::NotEvaluated
        ));
    }
}
