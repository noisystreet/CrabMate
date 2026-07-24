//! 非分层回合入口：开局意图门控 → **`assess_turn_routing`** → 统一 driver。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。

use crabmate_agent::agent_turn::{
    AssessTurnRoutingParams, IntentGateSnapshot, TurnRouteDriver, TurnTopLevelDispatch,
    assess_turn_routing,
};

use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;

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
    let assessed = assess_turn_routing(AssessTurnRoutingParams {
        cfg: p.ctx.core.cfg.as_ref(),
        top_level: TurnTopLevelDispatch::NonHierarchical,
        intent_gate: intent_gate_snapshot_or_unknown(p),
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
        &[("non_hierarchical_turn_phase", entry_phase.as_str())],
    );
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = mode,
        non_hierarchical_turn_phase = entry_phase.as_str(),
        freeform_because = assessed.decision.freeform_because.as_deref(),
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        "dispatch_non_hierarchical_turn"
    );
    run_non_hierarchical_turn(entry_phase, p, per_coord).await
}
