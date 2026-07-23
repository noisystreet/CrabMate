//! 非分层统一 driver：[`NonHierarchicalTurnPhase`] → 外循环。

use crate::agent::agent_turn::turn_orchestration::NonHierarchicalTurnPhase;
use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;

/// 非分层回合统一入口（`ReAct` 外循环）。
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
        NonHierarchicalTurnPhase::PlannedStep(_) => {
            log::info!(target: "crabmate", "run_agent_turn: planned step turn (outer loop)");
            run_agent_outer_loop(p, per_coord).await
        }
    }
}
