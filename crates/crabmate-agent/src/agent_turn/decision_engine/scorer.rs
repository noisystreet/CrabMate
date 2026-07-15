use super::traits::{FactorId, FactorScore};
use super::types::{OrchestrationDecision, OrchestrationRoute};

use log::info;

/// 默认因子权重（总和为 1.0）。
#[derive(Debug, Clone)]
pub struct FactorWeights {
    pub intent: f32,
    pub complexity: f32,
    pub workspace: f32,
    pub history: f32,
    pub cost: f32,
}

impl Default for FactorWeights {
    fn default() -> Self {
        Self {
            intent: 0.35,
            complexity: 0.25,
            workspace: 0.20,
            history: 0.10,
            cost: 0.10,
        }
    }
}

impl FactorWeights {
    pub fn get(&self, id: FactorId) -> f32 {
        match id {
            FactorId::Intent => self.intent,
            FactorId::Complexity => self.complexity,
            FactorId::Workspace => self.workspace,
            FactorId::History => self.history,
            FactorId::Cost => self.cost,
        }
    }
}

/// 默认 staged 阈值：total_score ≥ 0.4 时走 Staged。
pub const DEFAULT_STAGED_THRESHOLD: f32 = 0.4;

/// 加权聚合因子得分并判定路由。
pub fn score_and_route(
    scores: Vec<FactorScore>,
    weights: &FactorWeights,
    threshold: f32,
) -> OrchestrationDecision {
    let breakdown: Vec<FactorScore> = scores
        .into_iter()
        .map(|s| FactorScore::new(s.factor, s.raw_score, weights.get(s.factor), s.detail))
        .collect();

    let total_score: f32 = breakdown.iter().map(|s| s.contribution).sum();
    let confidence = total_score;

    let route = if total_score >= threshold {
        OrchestrationRoute::Staged
    } else {
        OrchestrationRoute::ReAct
    };

    info!(
        target: "crabmate_decision",
        "scoring route={:?} total={:.3} threshold={:.3} breakdown={}",
        route,
        total_score,
        threshold,
        breakdown.iter().map(|s| format!("{}={:.3}", s.factor.as_str(), s.contribution)).collect::<Vec<_>>().join(" "),
    );

    OrchestrationDecision {
        route,
        confidence,
        score_breakdown: breakdown,
        total_score,
    }
}
