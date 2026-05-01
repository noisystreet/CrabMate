//! 终答 **`StopTurnPendingPlanConsistencyLlm`** 之后：侧向语义 LLM 的 **`PlanSemanticLlmOutcome` → 外层循环决策**（纯函数）。
//! 与 **`per_coord::final_plan_gate`** 的挂起态衔接；由 **`per_reflect_after_assistant`** 调用，便于单测。

use crate::agent::per_coord::{PerCoordinator, PlanRewriteExhaustedReason};
use crate::agent::per_plan_semantic_check::PlanSemanticLlmOutcome;
use crate::types::Message;

/// 侧向语义校验完成后，对 **R 步 / 外层循环** 的控制（不含 IO，不修改 `PerCoordinator`）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlanSemanticConsistencyReflectCtl {
    StopTurn,
    PlanRewriteExhausted {
        reason: PlanRewriteExhaustedReason,
    },
    /// 调用方须在 push 本条 user **之前**执行 **`PerCoordinator::increment_plan_rewrite_attempts`**（与历史行为一致）。
    ContinueOuterWithRewriteUser(Message),
}

/// 将侧向 LLM 输出映射为下一步动作；**不**调用 `increment_plan_rewrite_attempts`（由调用方在 `Continue` 分支处理）。
pub(crate) fn map_plan_semantic_llm_outcome_to_reflect_ctl(
    plan_rewrite_attempts: usize,
    plan_rewrite_max_attempts: usize,
    outcome: &PlanSemanticLlmOutcome,
) -> PlanSemanticConsistencyReflectCtl {
    if outcome.consistent {
        return PlanSemanticConsistencyReflectCtl::StopTurn;
    }
    if plan_rewrite_attempts >= plan_rewrite_max_attempts {
        return PlanSemanticConsistencyReflectCtl::PlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent,
        };
    }
    PlanSemanticConsistencyReflectCtl::ContinueOuterWithRewriteUser(
        PerCoordinator::plan_semantic_mismatch_rewrite_message_with_feedback(
            outcome.violation_codes.as_slice(),
            outcome.rationale.as_deref(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageContent;

    fn outcome(
        consistent: bool,
        codes: Vec<String>,
        rationale: Option<String>,
    ) -> PlanSemanticLlmOutcome {
        PlanSemanticLlmOutcome {
            consistent,
            violation_codes: codes,
            rationale,
        }
    }

    #[test]
    fn consistent_always_stops() {
        let o = outcome(true, vec![], None);
        assert_eq!(
            map_plan_semantic_llm_outcome_to_reflect_ctl(0, 3, &o),
            PlanSemanticConsistencyReflectCtl::StopTurn
        );
    }

    #[test]
    fn inconsistent_at_cap_exhausted() {
        let o = outcome(false, vec!["x".into()], None);
        assert!(matches!(
            map_plan_semantic_llm_outcome_to_reflect_ctl(3, 3, &o),
            PlanSemanticConsistencyReflectCtl::PlanRewriteExhausted {
                reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent
            }
        ));
    }

    #[test]
    fn inconsistent_below_cap_requests_rewrite_user() {
        let o = outcome(false, vec!["code_a".into()], Some("because".into()));
        let ctl = map_plan_semantic_llm_outcome_to_reflect_ctl(1, 3, &o);
        match ctl {
            PlanSemanticConsistencyReflectCtl::ContinueOuterWithRewriteUser(m) => {
                assert_eq!(m.role, "user");
                let t = match m.content {
                    Some(MessageContent::Text(s)) => s,
                    _ => panic!("expected text user"),
                };
                assert!(t.contains("code_a"));
                assert!(t.contains("because"));
            }
            _ => panic!("expected ContinueOuterWithRewriteUser"),
        }
    }
}
