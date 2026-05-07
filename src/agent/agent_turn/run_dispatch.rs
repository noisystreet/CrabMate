//! 回合执行模式分发：分层 vs 非分层，以及非分层下的逻辑双代理 / 分阶段规划 / 单 Agent 外循环。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。
//!
//! **分阶段意图门控**：[`super::intent::assess_staged_planning_gate_full_pipeline`] 产出结构化 [`super::intent::StagedPlanningGateOutcome`]，
//! 与 `intent_pipeline::IntentDecision` 对齐，并与 **`intent_at_turn_start`** 共用 **L0+L1+可选 L2**。**[`execute_non_hierarchical_main_route`]** 将
//! [`super::turn_orchestration::NonHierarchicalEntryResolution`] 聚合门控与配置，给出显式 [`super::turn_orchestration::NonHierarchicalMainRoute`]。

use crate::agent::per_coord::PerCoordinator;

use super::errors::RunAgentTurnError;
use super::hierarchy;
use super::intent::{StagedPlanningGateOutcome, assess_staged_planning_gate_full_pipeline};
use super::intent_at_turn_start;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::staged::{
    run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps,
};
use super::turn_orchestration::{
    NonHierarchicalEntryResolution, NonHierarchicalMainRoute, TurnOrchestrationMode,
};

/// 执行非分层主路径（与 [`resolve_non_hierarchical_main_route`] 产物一一对应）。
pub(crate) async fn execute_non_hierarchical_main_route(
    main_route: NonHierarchicalMainRoute,
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    match main_route {
        NonHierarchicalMainRoute::LogicalDualAgentStaged => {
            log::info!(target: "crabmate", "run_agent_turn: using LogicalDualAgent mode");
            run_logical_dual_agent_then_execute_steps(p, per_coord).await
        }
        NonHierarchicalMainRoute::StagedPlanExecution => {
            log::info!(target: "crabmate", "run_agent_turn: using staged_plan mode");
            run_staged_plan_then_execute_steps(p, per_coord).await
        }
        NonHierarchicalMainRoute::SingleAgentOuterLoop => {
            log::info!(target: "crabmate", "run_agent_turn: using single_agent mode");
            run_agent_outer_loop(p, per_coord).await
        }
    }
}

/// `planner_executor_mode == Hierarchical`：意图门控在 [`hierarchy::run_hierarchical_agent`] 内完成。
pub(crate) async fn dispatch_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = TurnOrchestrationMode::Hierarchical.as_str(),
        "dispatch_hierarchical_turn"
    );
    log::info!(target: "crabmate", "run_agent_turn: using Hierarchical mode");
    hierarchy::run_hierarchical_agent(p).await
}

/// 非分层：开局意图门控 → 按配置选择逻辑双代理 / 分阶段规划 / 单 Agent 外循环。
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
    let entry = NonHierarchicalEntryResolution::resolve(p.ctx.core.cfg.as_ref(), &staged_gate);
    let main_route = entry.main_route;
    let mode = entry.orchestration_mode;
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = mode.as_str(),
        non_hierarchical_main_route = main_route.as_str(),
        staged_plan_intent_gate_allow = allow_staged,
        single_agent_outer_loop_because = entry
            .single_agent_outer_loop_because
            .map(|b| b.as_str()),
        planner_executor_mode = p.ctx.core.cfg.per_plan_policy.planner_executor_mode.as_str(),
        staged_plan_execution = p.ctx.core.cfg.staged_planning.staged_plan_execution,
        "dispatch_non_hierarchical_turn main_path"
    );
    execute_non_hierarchical_main_route(main_route, p, per_coord).await
}
