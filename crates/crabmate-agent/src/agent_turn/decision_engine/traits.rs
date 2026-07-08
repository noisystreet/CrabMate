use super::types::FactorContext;

/// 决策因子：输入上下文，输出 0.0–1.0 的评分。
pub trait DecisionFactor: std::fmt::Debug {
    /// 因子唯一标识，用于日志和配置。
    fn id(&self) -> FactorId;

    /// 评估因子，返回 0.0（完全反对 staged）到 1.0（强烈建议 staged）。
    fn evaluate(&self, ctx: &FactorContext) -> FactorScore;

    /// 默认权重（可通过配置覆盖）。
    fn default_weight(&self) -> f32;
}

/// 因子唯一标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactorId {
    Intent,
    Complexity,
    Workspace,
    History,
    Cost,
}

impl FactorId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Complexity => "complexity",
            Self::Workspace => "workspace",
            Self::History => "history",
            Self::Cost => "cost",
        }
    }
}

/// 单个因子的评估结果。
#[derive(Debug, Clone)]
pub struct FactorScore {
    pub factor: FactorId,
    pub raw_score: f32,
    pub weight: f32,
    pub contribution: f32,
    pub detail: String,
}

impl FactorScore {
    pub fn new(factor: FactorId, raw_score: f32, weight: f32, detail: String) -> Self {
        Self {
            factor,
            raw_score,
            weight,
            contribution: raw_score * weight,
            detail,
        }
    }
}
