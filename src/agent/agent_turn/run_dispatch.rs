//! 回合执行模式分发：分层 vs 非分层；非分层经 **`run_non_hierarchical_turn`** 统一 driver。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。
//!
//! **分阶段意图门控**：[`super::intent::assess_staged_planning_gate_full_pipeline`] 产出结构化 [`super::intent::StagedPlanningGateOutcome`]，
//! 与 `intent_pipeline::IntentDecision` 对齐，并与 **`intent_at_turn_start`** 共用 **L2 优先管线**。

use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;
use super::hierarchy;
use super::intent::{StagedPlanningGateOutcome, assess_staged_planning_gate_full_pipeline};
use super::intent_at_turn_start;
use super::non_hierarchical_turn::run_non_hierarchical_turn;
use super::orchestration_entry::{
    TurnOrchestrationTransition, log_orchestration_transition, resolve_non_hierarchical_turn,
};
use super::params::RunLoopParams;
use super::turn_orchestration::TurnOrchestrationMode;

/// `planner_executor_mode == Hierarchical`：意图门控在 [`hierarchy::run_hierarchical_agent`] 内完成。
pub(crate) async fn dispatch_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
        "dispatch_hierarchical_turn"
    );
    log::info!(target: "crabmate", "run_agent_turn: using Hierarchical mode");
    hierarchy::run_hierarchical_agent(p, per_coord).await
}

/// 非分层：开局意图门控 → 解析 [`NonHierarchicalTurnPhase`] → 统一 driver。
pub(crate) async fn dispatch_non_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    if !intent_at_turn_start::run_intent_at_turn_start_if_configured(p).await? {
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
    let entry = resolve_non_hierarchical_turn(p.ctx.core.cfg.as_ref(), &staged_gate);
    let turn_phase = entry.turn_phase;
    let mode = entry.orchestration_mode;
    log_orchestration_transition(
        TurnOrchestrationTransition::NonHierarchicalEntryResolved,
        Some(mode.as_str()),
        &[
            ("non_hierarchical_turn_phase", turn_phase.as_str()),
            (
                "freeform_because",
                entry.freeform_because.map(|b| b.as_str()).unwrap_or(""),
            ),
        ],
    );
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = mode.as_str(),
        non_hierarchical_turn_phase = turn_phase.as_str(),
        staged_plan_intent_gate_allow = allow_staged,
        freeform_because = entry.freeform_because.map(|b| b.as_str()),
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        "dispatch_non_hierarchical_turn"
    );
    run_non_hierarchical_turn(turn_phase, p, per_coord).await
}
