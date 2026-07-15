//! 非分层统一 driver：[`NonHierarchicalTurnPhase`] → 外循环或规划步滚动视界。

use crate::agent::agent_turn::turn_orchestration::{NonHierarchicalTurnPhase, PlannedStepKind};
use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::staged::{
    run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps,
};

/// 非分层回合统一入口（`ReAct` 外循环或 `PlannedStep` 滚动视界）。
pub(crate) async fn run_non_hierarchical_turn(
    phase: NonHierarchicalTurnPhase,
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    match phase {
        NonHierarchicalTurnPhase::ReAct => {
            log::info!(target: "crabmate", "run_agent_turn: react turn (outer loop)");
            run_agent_outer_loop(p, per_coord).await
        }
        NonHierarchicalTurnPhase::PlannedStep(kind) => {
            log::info!(
                target: "crabmate",
                "run_agent_turn: planned step turn ({})",
                kind.as_str()
            );
            match kind {
                PlannedStepKind::LogicalDual => {
                    run_logical_dual_agent_then_execute_steps(p, per_coord).await
                }
                PlannedStepKind::SingleAgent => {
                    run_staged_plan_then_execute_steps(p, per_coord).await
                }
            }
        }
    }
}
