//! 单轮编排路由决议快照（v1 JSON）：门控链结束后一次性记录，供 tracing / SSE / 金样回归。

use crabmate_config::{AgentConfig, FinalPlanRequirementMode};

use crate::intent_pipeline::{IntentAction, IntentDecision};
use crate::intent_router::IntentKind;

use super::orchestration_entry::TurnTopLevelDispatch;
use super::staged_planning_gate_types::{StagedPlanningDenyReason, StagedPlanningGateOutcome};
use super::turn_orchestration::{
    NonHierarchicalTurnPhase, NonHierarchicalTurnResolution, TurnOrchestrationMode,
};

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
        orchestration_profile: Some("react".to_string()),
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
        orchestration_profile: Some("react".to_string()),
    }
}

#[allow(dead_code)]
fn intent_decision_from_gate_snapshot(intent_gate: &IntentGateSnapshot) -> Option<IntentDecision> {
    match intent_gate {
        IntentGateSnapshot::ProceedExecute {
            kind,
            primary_intent,
            action,
            confidence,
            need_clarification,
            ..
        } => Some(IntentDecision {
            kind: match kind.as_str() {
                "execute" => IntentKind::Execute,
                "greeting" => IntentKind::Greeting,
                "qa" => IntentKind::Qa,
                _ => IntentKind::Ambiguous,
            },
            primary_intent: primary_intent.clone(),
            secondary_intents: Vec::new(),
            confidence: *confidence,
            abstain: false,
            need_clarification: *need_clarification,
            action: match action.as_str() {
                "execute" => IntentAction::Execute,
                "direct_reply" => IntentAction::DirectReply(String::new()),
                "clarify_then_execute" => IntentAction::ClarifyThenExecute(String::new()),
                "confirm_then_execute" => IntentAction::ConfirmThenExecute(String::new()),
                _ => IntentAction::Execute,
            },
            multi_intent: None,
        }),
        _ => None,
    }
}

/// 按 `OrchestrationProfile::ReAct`（唯一档位）覆盖分阶段门控结果：若门控放行则强制走 ReAct。
pub fn apply_orchestration_profile_to_staged_gate(
    _intent_gate: &IntentGateSnapshot,
    gate: &StagedPlanningGateOutcome,
) -> StagedPlanningGateOutcome {
    if gate.allows_staged_planning() {
        StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::OrchestrationProfileFreeform,
            task_preview: match gate {
                StagedPlanningGateOutcome::Allow { task_preview, .. } => Some(task_preview.clone()),
                StagedPlanningGateOutcome::Deny { task_preview, .. } => task_preview.clone(),
            },
            intent_decision: match gate {
                StagedPlanningGateOutcome::Allow { decision, .. } => Some(decision.clone()),
                StagedPlanningGateOutcome::Deny {
                    intent_decision, ..
                } => intent_decision.clone(),
            },
        }
    } else {
        gate.clone()
    }
}

/// 门控链收敛后的下一执行 driver（纯数据；IO 由 `run_dispatch` 执行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRouteDriver {
    /// **`intent_at_turn_start`** 已写入终答，本回合不再进入主执行。
    IntentEarlyExit,
    /// 非分层：外循环或分阶段滚动视界。
    NonHierarchical(NonHierarchicalTurnPhase),
}

/// [`assess_turn_routing`] 聚合输出：决议快照 + driver。
#[derive(Debug, Clone, PartialEq)]
pub struct AssessedTurnRoute {
    pub decision: TurnRouteDecisionV1,
    pub driver: TurnRouteDriver,
}

/// 纯函数入参：门控链结束后一次性评估（P0 取消/安全在更外层处理）。
#[derive(Debug)]
pub struct AssessTurnRoutingParams<'a> {
    pub cfg: &'a AgentConfig,
    pub top_level: TurnTopLevelDispatch,
    pub intent_gate: IntentGateSnapshot,
    /// 非分层必填；分层为 `None`。
    pub staged_gate: Option<&'a StagedPlanningGateOutcome>,
    /// 分层 **`ProceedExecute`** 必填；非分层可 `None`。
    pub hierarchical_decision: Option<&'a IntentDecision>,
}

#[inline]
pub fn intent_gate_is_early_exit(intent_gate: &IntentGateSnapshot) -> bool {
    matches!(intent_gate, IntentGateSnapshot::FinishedEarly { .. })
}

/// 非分层路由决议：intent 早退 → staged deny/allow。
pub fn assess_turn_routing(params: AssessTurnRoutingParams<'_>) -> AssessedTurnRoute {
    if intent_gate_is_early_exit(&params.intent_gate) {
        let decision = build_non_hierarchical_intent_finished_early_decision(
            params.cfg,
            params.intent_gate.clone(),
        );
        return AssessedTurnRoute {
            decision,
            driver: TurnRouteDriver::IntentEarlyExit,
        };
    }

    let staged_gate_raw = params
        .staged_gate
        .expect("non_hierarchical requires StagedPlanningGateOutcome");
    let staged_gate =
        apply_orchestration_profile_to_staged_gate(&params.intent_gate, staged_gate_raw);
    let entry = super::orchestration_entry::resolve_non_hierarchical_turn(params.cfg, &staged_gate);
    let decision = build_non_hierarchical_turn_route_decision(
        params.cfg,
        params.intent_gate.clone(),
        staged_gate_snapshot_from_outcome(&staged_gate),
        &entry,
    );
    AssessedTurnRoute {
        decision,
        driver: TurnRouteDriver::NonHierarchical(entry.turn_phase),
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
                multi_intent: None,
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
        assert_eq!(decision.orchestration_mode, "react");
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

    #[test]
    fn orchestration_profile_freeform_overrides_staged_allow() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = execute_gate_allow();
        let intent = IntentGateSnapshot::ProceedExecute {
            kind: "execute".into(),
            primary_intent: "execute.run_test_build".into(),
            action: "execute".into(),
            confidence: 0.95,
            need_clarification: false,
        };
        let adjusted = apply_orchestration_profile_to_staged_gate(&intent, &gate);
        assert!(!adjusted.allows_staged_planning());
        let assessed = assess_turn_routing(AssessTurnRoutingParams {
            cfg: &cfg,
            top_level: TurnTopLevelDispatch::NonHierarchical,
            intent_gate: intent,
            staged_gate: Some(&gate),
            hierarchical_decision: None,
        });
        assert_eq!(assessed.decision.orchestration_mode, "react");
        assert_eq!(
            assessed.decision.freeform_because.as_deref(),
            Some("orchestration_profile_freeform")
        );
    }
}
