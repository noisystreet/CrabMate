//! L0+L1+可选 L2 意图管线（纯合并 + 观测日志）；L2 LLM 调用经 [`IntentL2ClassifierHost`] 注入。

use async_trait::async_trait;
use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::agent_turn::intent::context::build_intent_routing_context;
use crate::intent_pipeline::{
    IntentDecision, IntentMergeMeta, L2IntentCandidate, assess_and_route_with_l2,
};
use crate::intent_router::ExecuteIntentThresholds;

/// L1/L2 合并后的意图判定与观测元数据。
#[derive(Debug, Clone)]
pub struct IntentRoutingOutcome {
    pub decision: IntentDecision,
    pub merge_meta: IntentMergeMeta,
}

/// [`assess_intent_routing_full_pipeline`] 入参（避免过多函数形参）。
pub struct IntentRoutingPipelineParams<'a> {
    pub task: &'a str,
    pub messages: &'a [Message],
    pub cfg: &'a AgentConfig,
    pub in_clarification_flow: bool,
    pub thresholds: ExecuteIntentThresholds,
    pub l2_enabled: bool,
    pub sse_log_tag: &'a str,
}

/// 根包实现的 L2 语义分类（无工具 LLM）；失败时返回 `None`（fail-open）。
#[async_trait]
pub trait IntentL2ClassifierHost: Send + Sync {
    async fn classify_l2_candidate(
        &self,
        routing_for_l1: &str,
        current_task: &str,
    ) -> Option<L2IntentCandidate>;
}

/// 在已有 L2 候选（或 `None`）时跑完整 L0+L1(+L2) 合并。
pub fn assess_intent_routing_with_optional_l2(
    task: &str,
    messages: &[Message],
    cfg: &AgentConfig,
    in_clarification_flow: bool,
    thresholds: ExecuteIntentThresholds,
    l2_candidate: Option<L2IntentCandidate>,
) -> IntentRoutingOutcome {
    let intent_ctx = build_intent_routing_context(messages, cfg, in_clarification_flow, thresholds);
    let (decision, merge_meta) = assess_and_route_with_l2(task, &intent_ctx, l2_candidate);
    IntentRoutingOutcome {
        decision,
        merge_meta,
    }
}

/// 结构化 info 日志（与根包 `at_turn_start` / `staged_planning_gate` 对齐）。
pub fn log_intent_pipeline_assessment(sse_log_tag: &str, outcome: &IntentRoutingOutcome) {
    let IntentRoutingOutcome {
        decision,
        merge_meta,
    } = outcome;
    log::info!(
        target: "crabmate",
        "[INTENT_PIPELINE] {sse_log_tag} l1_kind={:?} l1_confidence={:.2} l2_present={} l2_applied={} l2_confidence={:?} override={:?} final_kind={:?} primary={} conf={:.2} abstain={} need_clarif={} action={:?} merged_continuation={}",
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
}

/// 完整 L0+L1+可选 L2 管线：L2 经宿主注入，合并与日志在 crate 内完成。
pub async fn assess_intent_routing_full_pipeline<H: IntentL2ClassifierHost>(
    host: &H,
    params: &IntentRoutingPipelineParams<'_>,
) -> IntentRoutingOutcome {
    let IntentRoutingPipelineParams {
        task,
        messages,
        cfg,
        in_clarification_flow,
        thresholds,
        l2_enabled,
        sse_log_tag,
    } = params;
    let intent_ctx =
        build_intent_routing_context(messages, cfg, *in_clarification_flow, *thresholds);
    let (routing_for_l1, _, _) = crate::intent_pipeline::prepare_intent_routing(task, &intent_ctx);
    let l2_candidate = if *l2_enabled {
        host.classify_l2_candidate(&routing_for_l1, task).await
    } else {
        None
    };
    let outcome = assess_intent_routing_with_optional_l2(
        task,
        messages,
        cfg,
        *in_clarification_flow,
        *thresholds,
        l2_candidate,
    );
    log_intent_pipeline_assessment(sse_log_tag, &outcome);
    outcome
}
