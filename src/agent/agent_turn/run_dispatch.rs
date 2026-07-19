//! 非分层回合入口：开局意图门控 → **`assess_turn_routing`** → 统一 driver。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。
//!
//! **分阶段意图门控**：[`super::intent::assess_staged_planning_gate_full_pipeline`] 产出结构化 [`super::intent::StagedPlanningGateOutcome`]，
//! 与 `intent_pipeline::IntentDecision` 对齐，并与 **`intent_at_turn_start`** 共用 **L2 优先管线**。

use crabmate_agent::agent_turn::{
    AssessTurnRoutingParams, IntentGateSnapshot, TurnRouteDriver, TurnTopLevelDispatch,
    assess_turn_routing,
};

use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;
use super::intent::{StagedPlanningGateOutcome, assess_staged_planning_gate_full_pipeline};
use super::intent_at_turn_start;
use super::non_hierarchical_turn::run_non_hierarchical_turn;
use super::orchestration_entry::{TurnOrchestrationTransition, log_orchestration_transition};
use super::orchestration_route::record_and_emit_turn_route_decision;
use super::params::RunLoopParams;
use super::turn_orchestration::TurnOrchestrationMode;

fn intent_gate_snapshot_or_unknown(p: &RunLoopParams<'_>) -> IntentGateSnapshot {
    p.turn
        .turn_planner_hints
        .intent_gate_snapshot
        .clone()
        .unwrap_or(IntentGateSnapshot::Disabled)
}

/// 非分层：开局意图门控 → [`assess_turn_routing`] → 统一 driver。
pub(crate) async fn dispatch_non_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    if !intent_at_turn_start::run_intent_at_turn_start_if_configured(p).await? {
        let assessed = assess_turn_routing(AssessTurnRoutingParams {
            cfg: p.ctx.core.cfg.as_ref(),
            top_level: TurnTopLevelDispatch::NonHierarchical,
            intent_gate: intent_gate_snapshot_or_unknown(p),
            staged_gate: None,
            hierarchical_decision: None,
        });
        record_and_emit_turn_route_decision(p, &assessed.decision).await;
        tracing::info!(
            target: "crabmate::agent_turn",
            turn_orchestration_mode = TurnOrchestrationMode::IntentAtTurnStartFinished.as_str(),
            "dispatch_non_hierarchical_turn intent_at_turn_start finished"
        );
        log::info!(target: "crabmate", "run_agent_turn: intent_at_turn_start finished turn early");
        return Ok(());
    }
    let staged_gate = assess_staged_planning_gate_full_pipeline(p, "staged_plan_intent_gate").await;
    let allow_staged = staged_gate.allows_staged_planning();
    if !allow_staged && let StagedPlanningGateOutcome::Deny { reason, .. } = &staged_gate {
        tracing::debug!(
            target: "crabmate::agent_turn",
            staged_plan_intent_gate_allow = false,
            staged_plan_intent_gate_deny_reason = reason.as_str(),
            "staged_plan_intent_gate deny detail"
        );
    }
    let assessed = assess_turn_routing(AssessTurnRoutingParams {
        cfg: p.ctx.core.cfg.as_ref(),
        top_level: TurnTopLevelDispatch::NonHierarchical,
        intent_gate: intent_gate_snapshot_or_unknown(p),
        staged_gate: Some(&staged_gate),
        hierarchical_decision: None,
    });
    let entry_phase = match assessed.driver {
        TurnRouteDriver::NonHierarchical(phase) => phase,
        TurnRouteDriver::IntentEarlyExit => {
            record_and_emit_turn_route_decision(p, &assessed.decision).await;
            return Ok(());
        }
    };
    let mode = assessed.decision.orchestration_mode.as_str();
    record_and_emit_turn_route_decision(p, &assessed.decision).await;
    log_orchestration_transition(
        TurnOrchestrationTransition::NonHierarchicalEntryResolved,
        Some(mode),
        &[
            ("non_hierarchical_turn_phase", entry_phase.as_str()),
            (
                "freeform_because",
                assessed.decision.freeform_because.as_deref().unwrap_or(""),
            ),
        ],
    );
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = mode,
        non_hierarchical_turn_phase = entry_phase.as_str(),
        staged_plan_intent_gate_allow = allow_staged,
        freeform_because = assessed.decision.freeform_because.as_deref(),
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        "dispatch_non_hierarchical_turn"
    );
    run_non_hierarchical_turn(entry_phase, p, per_coord).await
}
