//! 回合执行模式分发：分层 vs 非分层，以及非分层下的逻辑双代理 / 分阶段规划 / 单 Agent 外循环。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。

use crate::agent::per_coord::PerCoordinator;
use crate::agent::{
    intent_l0,
    intent_pipeline::{IntentAction, IntentContext, assess_and_route},
    intent_router::ExecuteIntentThresholds,
};
use crate::config::PlannerExecutorMode;
use crate::types::Message;

use super::errors::RunAgentTurnError;
use super::hierarchy;
use super::intent_at_turn_start;
use super::intent_user;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::staged::{
    run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps,
};

const STAGED_INTENT_GATE_RECENT_USER_FOR_MERGE: usize = 4;
const STAGED_INTENT_GATE_MSG_TAIL_FOR_TOOL: usize = 32;

/// 是否允许本回合进入分阶段 / 逻辑双代理路径（与意图管线一致）。
pub(crate) fn should_enter_staged_planning(
    messages: &[Message],
    cfg: &crate::config::AgentConfig,
) -> bool {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(messages);
    let task = intent_user::extract_effective_user_task(messages, in_clarification_flow);
    if task.trim().is_empty() {
        return false;
    }
    let has_recent_tool_failure = intent_l0::messages_have_recent_tool_failure(
        messages,
        STAGED_INTENT_GATE_MSG_TAIL_FOR_TOOL,
    );
    let recent_user_messages = intent_user::collect_recent_user_messages(
        messages,
        STAGED_INTENT_GATE_RECENT_USER_FOR_MERGE,
    );
    let intent_ctx = IntentContext {
        recent_user_messages,
        in_clarification_flow,
        thresholds: ExecuteIntentThresholds {
            low: cfg.intent_non_hier_execute_low_threshold,
            high: cfg.intent_non_hier_execute_high_threshold,
        },
        l2_min_confidence: cfg.intent_l2_min_confidence,
        has_recent_tool_failure,
        l0_routing_boost_enabled: cfg.intent_l0_routing_boost_enabled,
    };
    let decision = assess_and_route(task.as_str(), &intent_ctx);
    let allowed = matches!(decision.action, IntentAction::Execute);
    log::info!(
        target: "crabmate",
        "staged_plan_intent_gate task_preview={} kind={:?} primary={} action={:?} allowed={}",
        crate::redact::preview_chars(task.as_str(), 80),
        decision.kind,
        decision.primary_intent,
        decision.action,
        allowed
    );
    allowed
}

/// `planner_executor_mode == Hierarchical`：意图门控在 [`hierarchy::run_hierarchical_agent`] 内完成。
pub(crate) async fn dispatch_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
) -> Result<(), RunAgentTurnError> {
    log::info!(target: "crabmate", "run_agent_turn: using Hierarchical mode");
    hierarchy::run_hierarchical_agent(p).await
}

/// 非分层：开局意图门控 → 按配置选择逻辑双代理 / 分阶段规划 / 单 Agent 外循环。
pub(crate) async fn dispatch_non_hierarchical_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), RunAgentTurnError> {
    if !intent_at_turn_start::run_intent_at_turn_start_if_configured(p).await? {
        return Ok(());
    }
    let allow_staged = should_enter_staged_planning(p.messages, p.cfg.as_ref());
    if p.cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent && allow_staged {
        log::info!(target: "crabmate", "run_agent_turn: using LogicalDualAgent mode");
        run_logical_dual_agent_then_execute_steps(p, per_coord).await
    } else if p.cfg.staged_plan_execution && allow_staged {
        log::info!(target: "crabmate", "run_agent_turn: using staged_plan mode");
        run_staged_plan_then_execute_steps(p, per_coord).await
    } else {
        log::info!(target: "crabmate", "run_agent_turn: using single_agent mode");
        run_agent_outer_loop(p, per_coord).await
    }
}
