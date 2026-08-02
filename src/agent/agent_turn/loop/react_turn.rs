//! ReAct 统一 driver：直接进入外循环。

use crate::agent::per_coord::PerCoordinator;

use super::outer_loop::run_agent_outer_loop;
use crate::agent::agent_turn::errors::RunAgentTurnError;
use crate::agent::agent_turn::params::RunLoopParams;

/// ReAct 回合统一入口（外循环）。
pub(crate) async fn run_react_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    log::info!(target: "crabmate", "run_agent_turn: react turn (outer loop)");
    run_agent_outer_loop(p, per_coord).await
}
