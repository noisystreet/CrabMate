pub mod factors;
pub mod scorer;
pub mod traits;
pub mod types;

use factors::FactorRegistry;
use factors::intent_factor::IntentFactor;
use scorer::{DEFAULT_STAGED_THRESHOLD, FactorWeights, score_and_route};
use traits::FactorScore;
use types::{FactorContext, OrchestrationDecision};

/// 编排决策引擎。
///
/// 维护因子注册表，对外提供 `evaluate()` 入口。
#[derive(Debug, Default)]
pub struct DecisionEngine {
    registry: FactorRegistry,
    weights: FactorWeights,
    threshold: f32,
}

impl DecisionEngine {
    /// 创建仅包含 `IntentFactor` 的引擎（Phase 1 行为与当前一致）。
    pub fn with_intent_only() -> Self {
        let mut registry = FactorRegistry::new();
        registry.register(Box::new(IntentFactor));
        Self {
            registry,
            weights: FactorWeights {
                intent: 1.0,
                ..FactorWeights::default()
            },
            threshold: DEFAULT_STAGED_THRESHOLD,
        }
    }

    /// 评估所有因子并返回决策。
    pub fn evaluate(&self, ctx: &FactorContext) -> OrchestrationDecision {
        let scores: Vec<FactorScore> = self.registry.evaluate_all(ctx);
        score_and_route(scores, &self.weights, self.threshold)
    }

    /// 查询当前因子权重。
    pub fn weights(&self) -> &FactorWeights {
        &self.weights
    }

    /// 查询当前阈值。
    pub fn threshold(&self) -> f32 {
        self.threshold
    }
}

/// 便捷构造：创建仅含 `IntentFactor` 的引擎并评估。
///
/// 这是 Phase 1 的入口：行为与 `staged_plan_eligibility_for_intent` 完全一致。
pub fn evaluate_intent_only(ctx: &FactorContext) -> OrchestrationDecision {
    let engine = DecisionEngine::with_intent_only();
    engine.evaluate(ctx)
}

#[cfg(test)]
mod tests {
    use super::types::OrchestrationRoute;
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;

    fn make_decision(action: IntentAction) -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action,
        }
    }

    fn make_ctx<'a>(decision: &'a IntentDecision) -> FactorContext<'a> {
        FactorContext {
            decision,
            task: "test task",
            messages: &[],
            cfg: None,
            workspace_file_count: None,
        }
    }

    #[test]
    fn intent_only_execute_routes_to_staged() {
        let decision = make_decision(IntentAction::Execute);
        let ctx = make_ctx(&decision);
        let result = evaluate_intent_only(&ctx);
        assert_eq!(result.route, OrchestrationRoute::Staged);
        assert!(result.total_score > 0.0);
        assert_eq!(result.score_breakdown.len(), 1);
        assert_eq!(result.score_breakdown[0].raw_score, 1.0);
    }

    #[test]
    fn intent_only_direct_reply_routes_to_freeform() {
        let decision = make_decision(IntentAction::DirectReply("hello".to_string()));
        let ctx = make_ctx(&decision);
        let result = evaluate_intent_only(&ctx);
        assert_eq!(result.route, OrchestrationRoute::Freeform);
        assert_eq!(result.total_score, 0.0);
        assert_eq!(result.score_breakdown[0].raw_score, 0.0);
    }

    #[test]
    fn intent_only_clarify_routes_to_freeform() {
        let decision = make_decision(IntentAction::ClarifyThenExecute("what?".to_string()));
        let ctx = make_ctx(&decision);
        let result = evaluate_intent_only(&ctx);
        assert_eq!(result.route, OrchestrationRoute::Freeform);
    }

    #[test]
    fn intent_only_confirm_routes_to_freeform() {
        let decision = make_decision(IntentAction::ConfirmThenExecute("sure?".to_string()));
        let ctx = make_ctx(&decision);
        let result = evaluate_intent_only(&ctx);
        assert_eq!(result.route, OrchestrationRoute::Freeform);
    }
}
