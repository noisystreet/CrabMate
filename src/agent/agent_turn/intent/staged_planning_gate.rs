//! 非分层模式下 **`resolve_non_hierarchical_turn_phase`** 之前的分阶段意图门控：
//! 与 [`super::at_turn_start::run_intent_l0_l1_l2_gate`] **共用**同一套 L2 优先管线（见 **`assess_staged_planning_gate_full_pipeline`**）。
//!
//! 同步 **L1** 门控与资格判定实现于 **`crabmate-agent::agent_turn::staged_planning_gate`**。

use crate::agent::intent_router::ExecuteIntentThresholds;
use crabmate_agent::agent_turn::{
    IntentRoutingOutcome, IntentRoutingPipelineParams, assess_intent_routing_full_pipeline,
    staged_planning_gate_outcome_from_decision,
};
pub(crate) use crabmate_agent::agent_turn::{StagedPlanningDenyReason, StagedPlanningGateOutcome};

use super::at_turn_start::emit_intent_timeline_gate_only;
use super::intent_user;
use super::l2_classifier_host::CrabmateIntentL2ClassifierHost;

/// 评估本回合是否允许进入分阶段 / 逻辑双代理路径（默认 L2，弃用规则层兜底，与非分层开局门控对齐）。
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

    let outcome = if let Some(cache) = p
        .turn
        .turn_planner_hints
        .intent_routing_cache_for_task(&task)
    {
        log::info!(
            target: "crabmate",
            "{sse_log_tag} intent_routing_cache_hit task_preview={}",
            crate::redact::preview_chars(task.as_str(), 80)
        );
        IntentRoutingOutcome {
            decision: cache.decision.clone(),
            merge_meta: cache.merge_meta.clone(),
        }
    } else {
        let host = CrabmateIntentL2ClassifierHost {
            cfg: p.ctx.core.cfg.as_ref(),
            llm_backend: p.ctx.core.llm_backend,
            client: p.ctx.core.client,
            api_key: p.ctx.core.api_key,
            turn_budget: Some(&p.turn.turn_budget),
        };
        assess_intent_routing_full_pipeline(
            &host,
            &IntentRoutingPipelineParams {
                task: task.as_str(),
                messages: p.turn.messages(),
                cfg: p.ctx.core.cfg.as_ref(),
                in_clarification_flow,
                thresholds: ExecuteIntentThresholds {
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
                l2_enabled: p.ctx.core.cfg.intent_routing.intent_l2_enabled,
                sse_log_tag,
            },
        )
        .await
    };

    let suppress_timeline = p.turn.take_suppress_duplicate_intent_timeline_once();
    if !suppress_timeline {
        emit_intent_timeline_gate_only(
            p.ctx.io.out,
            sse_log_tag,
            &outcome.decision,
            &outcome.merge_meta,
        )
        .await;
    }

    staged_planning_gate_outcome_from_decision(task, outcome.decision, p.ctx.core.cfg, sse_log_tag)
}

/// 同步门控（仅 **L1**，无 L2）；用于单测与无需 LLM 的探测。
#[cfg(test)]
pub(crate) fn assess_staged_planning_gate(
    messages: &[crate::types::Message],
    cfg: &crate::config::AgentConfig,
) -> StagedPlanningGateOutcome {
    crabmate_agent::agent_turn::assess_staged_planning_gate_l1(messages, cfg)
}
