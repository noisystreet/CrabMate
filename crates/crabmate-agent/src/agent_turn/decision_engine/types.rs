use crabmate_config::AgentConfig;
use crabmate_types::Message;

use crate::intent_pipeline::IntentDecision;

use super::traits::FactorScore;

/// 因子评估所需的只读上下文。
pub struct FactorContext<'a> {
    pub decision: &'a IntentDecision,
    pub task: &'a str,
    pub messages: &'a [Message],
    pub cfg: Option<&'a AgentConfig>,
    pub workspace_file_count: Option<usize>,
}

/// 决策引擎输出。
#[derive(Debug, Clone)]
pub struct OrchestrationDecision {
    pub route: OrchestrationRoute,
    pub confidence: f32,
    pub score_breakdown: Vec<FactorScore>,
    pub total_score: f32,
}

/// 编排路由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationRoute {
    ReAct,
    Staged,
}

impl OrchestrationRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
            Self::Staged => "staged",
        }
    }
}
