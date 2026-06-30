//! 终答门控一步转移的**决策原因**（仅 `tracing` / 单测；与 [`super::final_plan_gate::FinalPlanGateRoute`] 正交细化）。

/// 终答门控一步转移的**决策原因**（仅 `tracing` / 单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalPlanGateDecisionReason {
    PolicyNoRequirement,
    StructuredPlanAccepted,
    PendingSemanticConsistencyLlm,
    SemanticConsistencyAccepted,
    SemanticInconsistencyRewrite,
    SemanticInconsistencyRewriteExhausted,
    StaticSemanticsFailed,
    PlanParseFailed,
    PlanRewriteExhausted,
    UnexpectedPendingSemanticOnFinalArrived,
}

impl FinalPlanGateDecisionReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PolicyNoRequirement => "policy_no_requirement",
            Self::StructuredPlanAccepted => "structured_plan_accepted",
            Self::PendingSemanticConsistencyLlm => "pending_semantic_consistency_llm",
            Self::SemanticConsistencyAccepted => "semantic_consistency_accepted",
            Self::SemanticInconsistencyRewrite => "semantic_inconsistency_rewrite",
            Self::SemanticInconsistencyRewriteExhausted => {
                "semantic_inconsistency_rewrite_exhausted"
            }
            Self::StaticSemanticsFailed => "static_semantics_failed",
            Self::PlanParseFailed => "plan_parse_failed",
            Self::PlanRewriteExhausted => "plan_rewrite_exhausted",
            Self::UnexpectedPendingSemanticOnFinalArrived => {
                "unexpected_pending_semantic_on_final_arrived"
            }
        }
    }
}
