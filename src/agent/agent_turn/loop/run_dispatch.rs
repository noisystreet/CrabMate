//! 非分层回合入口：session_mode + Act 句启发式 → **`assess_turn_routing`** → ReAct。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。

use crabmate_agent::agent_turn::{
    AssessTurnRoutingParams, IntentGateSnapshot, TurnRouteDriver, TurnTopLevelDispatch,
    assess_turn_routing,
};

use crate::agent::per_coord::PerCoordinator;

use crate::agent::agent_turn::errors::RunAgentTurnError;

use super::non_hierarchical_turn::run_non_hierarchical_turn;
use super::orchestration_entry::{TurnOrchestrationTransition, log_orchestration_transition};
use super::orchestration_route::record_and_emit_turn_route_decision;
use crate::agent::agent_turn::intent_at_turn_start;
use crate::agent::agent_turn::params::RunLoopParams;

fn intent_gate_snapshot_or_unknown(p: &RunLoopParams<'_>) -> IntentGateSnapshot {
    p.turn
        .turn_planner_hints
        .intent_gate_snapshot
        .clone()
        .unwrap_or(IntentGateSnapshot::Disabled)
}

/// 非分层：Act 句启发式 → [`assess_turn_routing`] → ReAct 外循环。
pub(crate) async fn dispatch_non_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    intent_at_turn_start::run_act_turn_start_heuristics(p);
    // 会话 Ask/Plan：须在启发式之后挂只读（Ask/Plan 跳过 Act 句启发式；只读由本处 mode 挂载）。
    if crate::session_mode_turn::session_mode_requires_readonly_tools(p.ctx.attach.session_mode) {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(crabmate_agent::plan_artifact::PlanStepExecutorKind::ReviewReadonly);
        tracing::info!(
            target: "crabmate::agent_turn",
            session_mode = %p.ctx.attach.session_mode,
            "session_mode applied ReviewReadonly after act heuristics"
        );
    }
    let assessed = assess_turn_routing(AssessTurnRoutingParams {
        cfg: p.ctx.core.cfg.as_ref(),
        top_level: TurnTopLevelDispatch::NonHierarchical,
        intent_gate: intent_gate_snapshot_or_unknown(p),
    });
    record_and_emit_turn_route_decision(p, &assessed.decision).await;

    match assessed.driver {
        TurnRouteDriver::NonHierarchical(entry_phase) => {
            let mode = assessed.decision.orchestration_mode.as_str();
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
    }
}
