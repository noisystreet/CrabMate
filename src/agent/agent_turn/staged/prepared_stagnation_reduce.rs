//! 步后重入规划 **停滞检测** → 无 IO 的 reduce（表驱动；IO 仍在 **`mod.rs`**）。

use super::plan_stagnation::{
    StagedPlanStagnationAction, evaluate_staged_plan_stagnation_after_step_round,
};
use crate::agent::plan_artifact::AgentReplyPlanV1;
use crate::types::Message;

/// 首轮解析后、进入 post-parse 管线前的停滞 reduce 输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreparedStagnationReduceAction {
    ContinuePostParse,
    StopExhausted,
    ReplanWithFeedback(String),
}

impl PreparedStagnationReduceAction {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::ContinuePostParse => "continue_post_parse",
            Self::StopExhausted => "stop_exhausted",
            Self::ReplanWithFeedback(_) => "replan_with_feedback",
        }
    }
}

pub(super) fn reduce_prepared_stagnation_after_parse(
    messages: &[Message],
    plan: &AgentReplyPlanV1,
    entered_from_step_execution_round: bool,
) -> PreparedStagnationReduceAction {
    match evaluate_staged_plan_stagnation_after_step_round(
        messages,
        plan,
        entered_from_step_execution_round,
    ) {
        None => PreparedStagnationReduceAction::ContinuePostParse,
        Some(StagedPlanStagnationAction::StopAfterRepeatedPlan) => {
            PreparedStagnationReduceAction::StopExhausted
        }
        Some(StagedPlanStagnationAction::ReplanWithFeedback(body)) => {
            PreparedStagnationReduceAction::ReplanWithFeedback(body)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::{PlanStepExecutorKind, PlanStepV1};

    fn plan_one() -> AgentReplyPlanV1 {
        AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".into(),
                description: "d".into(),
                workflow_node_id: None,
                executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            }],
            no_task: false,
        }
    }

    #[test]
    fn first_round_no_stagnation() {
        let plan = plan_one();
        assert_eq!(
            reduce_prepared_stagnation_after_parse(&[], &plan, false),
            PreparedStagnationReduceAction::ContinuePostParse
        );
    }
}
