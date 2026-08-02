//! 意图管线同步入口（无 L2 LLM）：fail-open / 确认续接 / L0 观测；供金样与可选调用方。

use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::agent_turn::intent::context::build_intent_routing_context;
use crate::intent_pipeline::{IntentDecision, IntentMergeMeta, assess_and_route_with_meta};
use crate::intent_router::ExecuteIntentThresholds;

/// 意图判定与观测元数据。
#[derive(Debug, Clone)]
pub struct IntentRoutingOutcome {
    pub decision: IntentDecision,
    pub merge_meta: IntentMergeMeta,
}

/// [`assess_intent_routing_pipeline`] 入参（避免过多函数形参）。
pub struct IntentRoutingPipelineParams<'a> {
    pub task: &'a str,
    pub messages: &'a [Message],
    pub cfg: &'a AgentConfig,
    pub in_clarification_flow: bool,
    pub thresholds: ExecuteIntentThresholds,
    pub sse_log_tag: &'a str,
}

/// 同步跑 fail-open / 确认续接管线并记结构化日志（无额外 chat）。
pub fn assess_intent_routing_pipeline(
    params: &IntentRoutingPipelineParams<'_>,
) -> IntentRoutingOutcome {
    let IntentRoutingPipelineParams {
        task,
        messages,
        cfg,
        in_clarification_flow,
        thresholds,
        sse_log_tag,
    } = params;
    let intent_ctx =
        build_intent_routing_context(messages, cfg, *in_clarification_flow, *thresholds);
    let (decision, merge_meta) = assess_and_route_with_meta(task, &intent_ctx);
    let outcome = IntentRoutingOutcome {
        decision,
        merge_meta,
    };
    log_intent_pipeline_assessment(sse_log_tag, &outcome);
    outcome
}

/// 结构化 info 日志（与根包 `at_turn_start` 历史字段对齐，便于观测）。
pub fn log_intent_pipeline_assessment(sse_log_tag: &str, outcome: &IntentRoutingOutcome) {
    let IntentRoutingOutcome {
        decision,
        merge_meta,
    } = outcome;
    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] {sse_log_tag} baseline_kind={:?} baseline_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} l2_unavailable_reason={:?} source={:?} final_kind={:?} primary={} conf={:.2} abstain={} need_clarif={} action={:?} merged_continuation={}",
        merge_meta.l1_kind,
        merge_meta.l1_confidence,
        merge_meta.l2_present,
        merge_meta.l2_applied,
        merge_meta.l2_confidence,
        merge_meta.l2_unavailable_reason,
        merge_meta.override_reason,
        decision.kind,
        decision.primary_intent,
        decision.confidence,
        decision.abstain,
        decision.need_clarification,
        decision.action,
        merge_meta.used_merged_continuation,
    );
}
