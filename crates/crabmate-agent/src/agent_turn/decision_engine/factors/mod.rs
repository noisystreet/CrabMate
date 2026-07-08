pub mod intent_factor;

use super::traits::{DecisionFactor, FactorId, FactorScore};
use super::types::FactorContext;
use std::collections::BTreeMap;

/// 因子注册表：按 `FactorId` 索引已注册的因子。
#[derive(Debug, Default)]
pub struct FactorRegistry {
    factors: BTreeMap<FactorId, Box<dyn DecisionFactor + Send + Sync>>,
}

impl FactorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factor: Box<dyn DecisionFactor + Send + Sync>) {
        let id = factor.id();
        self.factors.insert(id, factor);
    }

    pub fn evaluate_all(&self, ctx: &FactorContext) -> Vec<FactorScore> {
        self.factors.values().map(|f| f.evaluate(ctx)).collect()
    }
}
