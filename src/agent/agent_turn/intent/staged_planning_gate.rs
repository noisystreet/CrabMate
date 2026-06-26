//! 非分层模式下 **`resolve_non_hierarchical_main_route`** 之前的分阶段意图门控：
//! 与 [`super::at_turn_start::run_intent_l0_l1_l2_gate`] **共用**同一套 L0+L1+可选 L2 管线（见 **`assess_staged_planning_gate_full_pipeline`**），避免仅用 L1（**`assess_and_route`**）与开局门控分叉。
//!
//! 同步 **L1** 门控与资格判定实现于 **`crabmate-agent::agent_turn::staged_planning_gate`**。

use crate::agent::intent_l2_classifier::classify_intent_l2_with_llm;
use crate::agent::intent_router::ExecuteIntentThresholds;
use crabmate_agent::agent_turn::staged_planning_gate::staged_planning_gate_outcome_from_decision;
use crabmate_agent::intent_pipeline::{assess_and_route_with_l2, prepare_intent_routing};

use super::at_turn_start::emit_intent_timeline_gate_only;
use super::build_intent_routing_context;
use super::intent_user;
pub(crate) use crabmate_agent::agent_turn::{StagedPlanningDenyReason, StagedPlanningGateOutcome};

/// 评估本回合是否允许进入分阶段 / 逻辑双代理路径（完整 **L0+L1+可选 L2**，与非分层开局门控对齐）。
pub(crate) async fn assess_staged_planning_gate_full_pipeline(
    p: &mut crate::agent::agent_turn::params::RunLoopParams<'_>,
    sse_log_tag: &'static str,
) -> StagedPlanningGateOutcome {
    let in_clarification_flow =
        intent_user::recently_waiting_execute_confirmation(p.turn.messages());
    let task = intent_user::extract_effective_user_task(p.turn.messages(), in_clarification_flow);
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
        p.turn.messages(),
        p.ctx.core.cfg.as_ref(),
        in_clarification_flow,
        ExecuteIntentThresholds {
            low: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_low_threshold,
            high: p
                .ctx
                .core
                .cfg
                .intent_routing
                .intent_non_hier_execute_high_threshold,
        },
    );
    let (routing_for_l1, _, _) = prepare_intent_routing(task.as_str(), &intent_ctx);
    let l2_candidate = if p.ctx.core.cfg.intent_routing.intent_l2_enabled {
        classify_intent_l2_with_llm(
            &routing_for_l1,
            task.as_str(),
            p.ctx.core.cfg.as_ref(),
            p.ctx.core.llm_backend,
            p.ctx.core.client,
            p.ctx.core.api_key,
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

    let suppress_timeline = p.turn.take_suppress_duplicate_intent_timeline_once();
    if !suppress_timeline {
        emit_intent_timeline_gate_only(p.ctx.io.out, sse_log_tag, &decision, &merge_meta).await;
    }

    staged_planning_gate_outcome_from_decision(
        task,
        decision,
        &p.ctx.core.cfg.staged_planning,
        sse_log_tag,
    )
}

/// 同步门控（仅 **L1**，无 L2）；用于单测与无需 LLM 的探测。
#[cfg(test)]
pub(crate) fn assess_staged_planning_gate(
    messages: &[crate::types::Message],
    cfg: &crate::config::AgentConfig,
) -> StagedPlanningGateOutcome {
    crabmate_agent::agent_turn::assess_staged_planning_gate_l1(messages, cfg)
}
