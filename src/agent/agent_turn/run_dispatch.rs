//! 回合执行模式分发：分层 vs 非分层，以及非分层下的逻辑双代理 / 分阶段规划 / 单 Agent 外循环。
//!
//! 从 [`super::run_agent_turn_common`] 抽离，使 `mod.rs` 仅保留入口日志、分隔线与 `PerCoordinator` 构造等接线。
//!
//! **分阶段意图门控**：[`assess_staged_planning_gate`] 产出结构化 [`StagedPlanningGateOutcome`]，
//! 与 `intent_pipeline::IntentDecision` 对齐。**[`execute_non_hierarchical_main_route`]** 将
//! [`super::turn_orchestration::NonHierarchicalMainRoute`] 与三条执行路径一一对应。

use crate::agent::per_coord::PerCoordinator;
use crate::agent::{
    intent_pipeline::{IntentAction, IntentDecision, assess_and_route},
    intent_router::{ExecuteIntentThresholds, IntentKind},
};
use crate::types::Message;

use super::errors::RunAgentTurnError;
use super::hierarchy;
use super::intent::build_intent_routing_context;
use super::intent_at_turn_start;
use super::intent_user;
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;
use super::staged::{
    run_logical_dual_agent_then_execute_steps, run_staged_plan_then_execute_steps,
};
use super::turn_orchestration::{
    NonHierarchicalMainRoute, TurnOrchestrationMode, resolve_non_hierarchical_main_route,
};

/// 非分层路径下，是否允许进入分阶段 / 逻辑双代理编排（仅 `IntentAction::Execute` 为 true）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StagedPlanningGateOutcome {
    /// 意图管线判定为「执行任务」，可分流到 staged / logical dual。
    Allow {
        task_preview: String,
        intent_kind: IntentKind,
        primary_intent: String,
        confidence: f32,
        decision: IntentDecision,
    },
    /// 无可路由的有效 user 任务句，或管线未给出 Execute。
    Deny {
        reason: StagedPlanningDenyReason,
        task_preview: Option<String>,
        intent_decision: Option<IntentDecision>,
    },
}

/// 拒绝进入分阶段编排的原因（用于日志与单测；不含机密）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanningDenyReason {
    /// `extract_effective_user_task` 为空（无 user 或全文空白）。
    EmptyEffectiveTask,
    /// 管线已跑通，但 `action != Execute`（直接回复 / 澄清 / 确认等）。
    IntentPipelineNotExecute,
}

impl StagedPlanningDenyReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::EmptyEffectiveTask => "empty_effective_task",
            Self::IntentPipelineNotExecute => "intent_pipeline_not_execute",
        }
    }
}

impl StagedPlanningGateOutcome {
    pub(crate) fn allows_staged_planning(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

fn intent_action_discriminant(action: &IntentAction) -> &'static str {
    match action {
        IntentAction::Execute => "execute",
        IntentAction::DirectReply(_) => "direct_reply",
        IntentAction::ClarifyThenExecute(_) => "clarify_then_execute",
        IntentAction::ConfirmThenExecute(_) => "confirm_then_execute",
    }
}

/// 评估本回合是否允许进入分阶段 / 逻辑双代理路径。
pub(crate) fn assess_staged_planning_gate(
    messages: &[Message],
    cfg: &crate::config::AgentConfig,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(messages);
    let task = intent_user::extract_effective_user_task(messages, in_clarification_flow);
    if task.trim().is_empty() {
        log::info!(
            target: "crabmate",
            "staged_plan_intent_gate outcome=deny reason=empty_effective_task"
        );
        return StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
    }

    let intent_ctx = build_intent_routing_context(
        messages,
        cfg,
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: cfg.intent_non_hier_execute_low_threshold,
            high: cfg.intent_non_hier_execute_high_threshold,
        },
    );
    let decision = assess_and_route(task.as_str(), &intent_ctx);
    let allowed = matches!(decision.action, IntentAction::Execute);

    log::info!(
        target: "crabmate",
        "staged_plan_intent_gate outcome={} reason={} task_preview={} kind={:?} primary={} action_discriminant={} confidence={:.3}",
        if allowed { "allow" } else { "deny" },
        if allowed {
            "execute_intent"
        } else {
            "intent_pipeline_not_execute"
        },
        crate::redact::preview_chars(task.as_str(), 80),
        decision.kind,
        decision.primary_intent,
        intent_action_discriminant(&decision.action),
        decision.confidence
    );

    if allowed {
        StagedPlanningGateOutcome::Allow {
            task_preview: task,
            intent_kind: decision.kind,
            primary_intent: decision.primary_intent.clone(),
            confidence: decision.confidence,
            decision,
        }
    } else {
        StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::IntentPipelineNotExecute,
            task_preview: Some(task),
            intent_decision: Some(decision),
        }
    }
}

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
    let staged_gate = assess_staged_planning_gate(p.turn.messages, p.ctx.cfg.as_ref());
    let allow_staged = staged_gate.allows_staged_planning();
    if !allow_staged && let StagedPlanningGateOutcome::Deny { reason, .. } = &staged_gate {
        tracing::debug!(
            target: "crabmate::agent_turn",
            staged_plan_intent_gate_allow = false,
            staged_plan_intent_gate_deny_reason = reason.as_str(),
            "staged_plan_intent_gate deny detail"
        );
    }
    let main_route = resolve_non_hierarchical_main_route(p.ctx.cfg.as_ref(), allow_staged);
    let mode: TurnOrchestrationMode = main_route.into();
    tracing::info!(
        target: "crabmate::agent_turn",
        turn_orchestration_mode = mode.as_str(),
        non_hierarchical_main_route = main_route.as_str(),
        staged_plan_intent_gate_allow = allow_staged,
        planner_executor_mode = p.ctx.cfg.planner_executor_mode.as_str(),
        staged_plan_execution = p.ctx.cfg.staged_plan_execution,
        "dispatch_non_hierarchical_turn main_path"
    );
    execute_non_hierarchical_main_route(main_route, p, per_coord).await
}
