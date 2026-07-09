pub mod factors;
pub mod scorer;
pub mod traits;
pub mod types;

use factors::FactorRegistry;
use factors::complexity_factor::ComplexityFactor;
use factors::intent_factor::IntentFactor;
use scorer::{DEFAULT_STAGED_THRESHOLD, FactorWeights, score_and_route};
use traits::FactorScore;
use types::{FactorContext, OrchestrationDecision};

/// 决策引擎模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecisionEngineMode {
    /// 保持现有行为（仅 IntentFactor，等价于 Phase 1）。
    #[default]
    Auto,
    /// 多因子评分（IntentFactor + ComplexityFactor 等）。
    Scored,
}

/// 编排决策引擎。
///
/// 维护因子注册表，对外提供 `evaluate()` 入口。
#[derive(Debug, Default)]
pub struct DecisionEngine {
    registry: FactorRegistry,
    weights: FactorWeights,
    threshold: f32,
    mode: DecisionEngineMode,
}

impl DecisionEngine {
    /// 根据模式构建引擎。
    pub fn build(mode: DecisionEngineMode, weights: FactorWeights, threshold: f32) -> Self {
        let mut registry = FactorRegistry::new();
        registry.register(Box::new(IntentFactor));
        if matches!(mode, DecisionEngineMode::Scored) {
            registry.register(Box::new(ComplexityFactor));
        }
        Self {
            registry,
            weights,
            threshold,
            mode,
        }
    }

    /// 创建仅包含 `IntentFactor` 的引擎（Phase 1 行为与当前一致）。
    pub fn with_intent_only() -> Self {
        Self::build(
            DecisionEngineMode::Auto,
            FactorWeights {
                intent: 1.0,
                ..FactorWeights::default()
            },
            DEFAULT_STAGED_THRESHOLD,
        )
    }

    /// 评估所有因子并返回决策。
    pub fn evaluate(&self, ctx: &FactorContext) -> OrchestrationDecision {
        let scores: Vec<FactorScore> = self.registry.evaluate_all(ctx);
        score_and_route(scores, &self.weights, self.threshold)
    }

    pub fn mode(&self) -> DecisionEngineMode {
        self.mode
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

/// Phase 2 入口：创建含 `IntentFactor` + `ComplexityFactor` 的引擎并评估。
pub fn evaluate_scored(ctx: &FactorContext) -> OrchestrationDecision {
    let engine = DecisionEngine::build(
        DecisionEngineMode::Scored,
        FactorWeights::default(),
        DEFAULT_STAGED_THRESHOLD,
    );
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
            multi_intent: None,
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

    #[test]
    fn scored_mode_registers_two_factors() {
        let engine = DecisionEngine::build(
            DecisionEngineMode::Scored,
            FactorWeights::default(),
            DEFAULT_STAGED_THRESHOLD,
        );
        let decision = make_decision(IntentAction::Execute);
        let ctx = make_ctx(&decision);
        let result = engine.evaluate(&ctx);
        assert_eq!(result.score_breakdown.len(), 2);
    }

    #[test]
    fn auto_mode_registers_only_intent_factor() {
        let engine = DecisionEngine::build(
            DecisionEngineMode::Auto,
            FactorWeights::default(),
            DEFAULT_STAGED_THRESHOLD,
        );
        let decision = make_decision(IntentAction::Execute);
        let ctx = make_ctx(&decision);
        let result = engine.evaluate(&ctx);
        assert_eq!(result.score_breakdown.len(), 1);
    }

    #[test]
    fn scored_simple_task_routes_freeform() {
        let engine = DecisionEngine::build(
            DecisionEngineMode::Scored,
            FactorWeights::default(),
            DEFAULT_STAGED_THRESHOLD,
        );
        let decision = make_decision(IntentAction::Execute);
        let ctx = FactorContext {
            decision: &decision,
            task: "cargo build",
            messages: &[],
            cfg: None,
            workspace_file_count: None,
        };
        let result = engine.evaluate(&ctx);
        // intent=0.35 + complexity≈0.0 = 0.35 < 0.4 → Freeform
        assert_eq!(result.route, OrchestrationRoute::Freeform);
    }

    #[test]
    fn scored_complex_task_routes_staged() {
        let engine = DecisionEngine::build(
            DecisionEngineMode::Scored,
            FactorWeights::default(),
            DEFAULT_STAGED_THRESHOLD,
        );
        let decision = make_decision(IntentAction::Execute);
        let long_task = "refactor the authentication module in src/auth.rs, \
            src/auth/handler.rs, src/auth/middleware.rs to use the new token \
            validation flow and add comprehensive unit tests for all edge cases";
        let ctx = FactorContext {
            decision: &decision,
            task: long_task,
            messages: &[],
            cfg: None,
            workspace_file_count: None,
        };
        let result = engine.evaluate(&ctx);
        // intent=0.35 + complexity>0.05 → >0.4 → Staged
        assert_eq!(result.route, OrchestrationRoute::Staged);
    }
}
