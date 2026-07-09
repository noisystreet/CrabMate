use crate::agent_turn::decision_engine::traits::{DecisionFactor, FactorId, FactorScore};
use crate::agent_turn::decision_engine::types::FactorContext;

/// 工作区因子：基于项目文件数评估复杂度。
///
/// 大项目（文件多）意味着 staged 分阶段规划收益更高。
/// - workspace_file_count ≥ 100 → 1.0
/// - workspace_file_count = 0    → 0.0（无信息时不偏向任何一方）
#[derive(Debug, Default)]
pub struct WorkspaceFactor;

impl WorkspaceFactor {
    const MAX_FILES: usize = 100;

    fn file_count_score(count: Option<usize>) -> f32 {
        match count {
            None => 0.0,
            Some(c) if c >= Self::MAX_FILES => 1.0,
            Some(c) => c as f32 / Self::MAX_FILES as f32,
        }
    }
}

impl DecisionFactor for WorkspaceFactor {
    fn id(&self) -> FactorId {
        FactorId::Workspace
    }

    fn evaluate(&self, ctx: &FactorContext) -> FactorScore {
        let raw = Self::file_count_score(ctx.workspace_file_count).clamp(0.0, 1.0);
        let detail = match ctx.workspace_file_count {
            Some(n) => format!("files={}", n),
            None => "no_data".to_string(),
        };
        FactorScore::new(self.id(), raw, self.default_weight(), detail)
    }

    fn default_weight(&self) -> f32 {
        0.20
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;

    fn make_decision() -> IntentDecision {
        IntentDecision {
            kind: IntentKind::Execute,
            primary_intent: "execute.code_change".to_string(),
            secondary_intents: vec![],
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action: IntentAction::Execute,
            multi_intent: None,
        }
    }

    fn ctx_with_file_count(count: Option<usize>) -> FactorContext<'static> {
        let decision = Box::leak(Box::new(make_decision()));
        let task = Box::leak(Box::new(String::from("test")));
        FactorContext {
            decision,
            task,
            messages: &[],
            cfg: None,
            workspace_file_count: count,
        }
    }

    #[test]
    fn no_data_returns_neutral() {
        let score = WorkspaceFactor.evaluate(&ctx_with_file_count(None));
        assert_eq!(score.raw_score, 0.0);
        assert!(score.detail.contains("no_data"));
    }

    #[test]
    fn empty_workspace_returns_zero() {
        let score = WorkspaceFactor.evaluate(&ctx_with_file_count(Some(0)));
        assert_eq!(score.raw_score, 0.0);
    }

    #[test]
    fn medium_workspace_scales() {
        let score = WorkspaceFactor.evaluate(&ctx_with_file_count(Some(50)));
        assert!((score.raw_score - 0.5).abs() < 0.01);
    }

    #[test]
    fn large_workspace_saturates() {
        let score = WorkspaceFactor.evaluate(&ctx_with_file_count(Some(100)));
        assert_eq!(score.raw_score, 1.0);
    }

    #[test]
    fn oversize_does_not_exceed_one() {
        let score = WorkspaceFactor.evaluate(&ctx_with_file_count(Some(200)));
        assert_eq!(score.raw_score, 1.0);
    }
}
