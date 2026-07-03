//! 步后重规划 **完成证据抑制** → 无 IO 的 reduce（表驱动；日志仍在调用方）。

use crate::agent::plan_artifact::PlanStepV1;
use crate::types::Message;

use super::super::turn_completion::turn_suppress_completed_replanning;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompletedReplanningReduceAction {
    ContinuePostParse,
    FinishQuiet,
}

impl CompletedReplanningReduceAction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::ContinuePostParse => "continue_post_parse",
            Self::FinishQuiet => "finish_quiet",
        }
    }
}

pub(super) fn reduce_completed_replanning_suppression(
    messages: &[Message],
    entered_from_step_execution_round: bool,
    steps: &[PlanStepV1],
) -> CompletedReplanningReduceAction {
    if turn_suppress_completed_replanning(messages, entered_from_step_execution_round, steps) {
        CompletedReplanningReduceAction::FinishQuiet
    } else {
        CompletedReplanningReduceAction::ContinuePostParse
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;

    fn step(id: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.into(),
            description: "verify".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: Some("verify".into()),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    #[test]
    fn no_suppress_on_empty_messages() {
        assert_eq!(
            reduce_completed_replanning_suppression(&[], true, &[step("x")]),
            CompletedReplanningReduceAction::ContinuePostParse
        );
    }
}
