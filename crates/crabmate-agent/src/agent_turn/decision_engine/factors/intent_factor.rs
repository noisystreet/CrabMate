use crate::agent_turn::decision_engine::traits::{DecisionFactor, FactorId, FactorScore};
use crate::agent_turn::decision_engine::types::FactorContext;
use crate::intent_pipeline::IntentAction;

/// 意图因子：迁移现有 `staged_plan_eligibility_for_intent` 逻辑。
///
/// - `IntentAction::Execute` → 1.0（完全赞同 staged）
/// - 其他 action → 0.0（不赞同 staged）
#[derive(Debug, Default)]
pub struct IntentFactor;

impl DecisionFactor for IntentFactor {
    fn id(&self) -> FactorId {
        FactorId::Intent
    }

    fn evaluate(&self, ctx: &FactorContext) -> FactorScore {
        let (raw_score, detail) = if matches!(ctx.decision.action, IntentAction::Execute) {
            (1.0, "execute_intent".to_string())
        } else {
            (0.0, "non_execute_action".to_string())
        };
        FactorScore::new(self.id(), raw_score, self.default_weight(), detail)
    }

    fn default_weight(&self) -> f32 {
        0.35
    }
}
