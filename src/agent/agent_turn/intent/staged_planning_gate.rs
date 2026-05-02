//! 非分层模式下 **`resolve_non_hierarchical_main_route`** 之前的分阶段意图门控：
//! 与 [`super::at_turn_start::run_intent_l0_l1_l2_gate`] **共用**同一套 L0+L1+可选 L2 管线（见 **`run_full_intent_pipeline_assessment`**），避免仅用 L1（**`assess_and_route`**）与开局门控分叉。
//!
//! 同步 **`assess_staged_planning_gate`**（仅 L1）保留供不需要异步/`RunLoopParams` 的单测与快速探测。

use crate::agent::intent_l2_classifier::classify_intent_l2_with_llm;
use crate::agent::intent_pipeline::{
    IntentAction, IntentDecision, assess_and_route_with_l2, prepare_intent_routing,
};
use crate::agent::intent_router::{ExecuteIntentThresholds, IntentKind};

#[cfg(test)]
use crate::agent::intent_pipeline::assess_and_route;
#[cfg(test)]
use crate::config::AgentConfig;
#[cfg(test)]
use crate::types::Message;

use super::at_turn_start::emit_intent_timeline_gate_only;
use super::build_intent_routing_context;
use super::intent_user;

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

fn log_staged_gate_outcome(
    allowed: bool,
    task: &str,
    decision: &IntentDecision,
    sse_tag: &'static str,
) {
    log::info!(
        target: "crabmate",
        "{sse_tag} outcome={} reason={} task_preview={} kind={:?} primary={} action_discriminant={} confidence={:.3}",
        if allowed { "allow" } else { "deny" },
        if allowed {
            "execute_intent"
        } else {
            "intent_pipeline_not_execute"
        },
        crate::redact::preview_chars(task, 80),
        decision.kind,
        decision.primary_intent,
        intent_action_discriminant(&decision.action),
        decision.confidence
    );
}

/// 评估本回合是否允许进入分阶段 / 逻辑双代理路径（完整 **L0+L1+可选 L2**，与非分层开局门控对齐）。
pub(crate) async fn assess_staged_planning_gate_full_pipeline(
    p: &mut crate::agent::agent_turn::params::RunLoopParams<'_>,
    sse_log_tag: &'static str,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow = intent_user::recently_waiting_execute_confirmation(p.turn.messages);
    let task = intent_user::extract_effective_user_task(p.turn.messages, in_clarification_flow);
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
        p.turn.messages,
        p.ctx.cfg.as_ref(),
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: p.ctx.cfg.intent_non_hier_execute_low_threshold,
            high: p.ctx.cfg.intent_non_hier_execute_high_threshold,
        },
    );
    let (routing_for_l1, _, _) = prepare_intent_routing(task.as_str(), &intent_ctx);
    let l2_candidate = if p.ctx.cfg.intent_l2_enabled {
        classify_intent_l2_with_llm(
            &routing_for_l1,
            task.as_str(),
            p.ctx.cfg.as_ref(),
            p.ctx.llm_backend,
            p.ctx.client,
            p.ctx.api_key,
        )
        .await
    } else {
        None
    };
    let (decision, merge_meta) = assess_and_route_with_l2(task.as_str(), &intent_ctx, l2_candidate);

    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] {sse_log_tag} staged_plan_intent_gate l1_kind={:?} l1_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} override={:?} final_kind={:?} primary={} conf={:.2} abstain={} need_clarif={} action={:?} merged_continuation={}",
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.override_reason,
        decision.kind,
        decision.primary_intent,
        decision.confidence,
        decision.abstain,
        decision.need_clarification,
        &decision.action,
        merge_meta.used_merged_continuation,
    );

    let suppress_timeline = p.turn.suppress_duplicate_intent_timeline_once;
    if suppress_timeline {
        p.turn.suppress_duplicate_intent_timeline_once = false;
    }
    if !suppress_timeline {
        emit_intent_timeline_gate_only(p.ctx.out, sse_log_tag, &decision, &merge_meta).await;
    }

    let allowed = matches!(decision.action, IntentAction::Execute);
    log_staged_gate_outcome(allowed, task.as_str(), &decision, sse_log_tag);

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

/// 同步门控（仅 **L1**，无 L2）；用于单测与无需 LLM 的探测。
#[cfg(test)]
pub(crate) fn assess_staged_planning_gate(
    messages: &[Message],
    cfg: &AgentConfig,
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

    log_staged_gate_outcome(
        allowed,
        task.as_str(),
        &decision,
        "staged_plan_intent_gate_sync",
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
