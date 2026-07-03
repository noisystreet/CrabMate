//! 步循环 **`StagedStepPostOuterRoute`** → 无 IO 的 reduce 动作（表驱动；IO 仍在 **`steps_loop`**）。

use super::step_iteration_fsm::StagedStepIterationCtl;
use super::steps_loop_route_fsm::StagedStepPostOuterRoute;

/// `resolve_staged_step_post_outer_route*` 之后的纯 reduce 输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepIterationReduceAction {
    ExecOrVerifyFailed,
    Cancelled,
    ToolFailurePatch,
    EmitSuccessAdvance,
}

impl StepIterationReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ExecOrVerifyFailed => "exec_or_verify_failed",
            Self::Cancelled => "cancelled",
            Self::ToolFailurePatch => "tool_failure_patch",
            Self::EmitSuccessAdvance => "emit_success_advance",
        }
    }
}

pub(crate) fn reduce_staged_step_post_outer_route(
    route: StagedStepPostOuterRoute,
) -> StepIterationReduceAction {
    match route {
        StagedStepPostOuterRoute::ExecOrVerifyFailed => {
            StepIterationReduceAction::ExecOrVerifyFailed
        }
        StagedStepPostOuterRoute::Cancelled => StepIterationReduceAction::Cancelled,
        StagedStepPostOuterRoute::ToolFailurePatch => StepIterationReduceAction::ToolFailurePatch,
        StagedStepPostOuterRoute::EmitSuccess => StepIterationReduceAction::EmitSuccessAdvance,
    }
}

/// 成功收尾时的迭代控制（与历史 **`StagedStepIterationCtl::AdvanceToNextStep`** 对齐）。
pub(crate) fn step_iteration_ctl_for_emit_success(
    n: usize,
    step_index: usize,
) -> StagedStepIterationCtl {
    StagedStepIterationCtl::AdvanceToNextStep {
        n,
        completed_steps: step_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};
    use crate::agent::agent_turn::staged::steps_loop_route_fsm::resolve_staged_step_post_outer_route_from_results;

    #[test]
    fn reduce_matches_post_outer_route_table() {
        let ok = Ok(());
        let cases = [
            (
                resolve_staged_step_post_outer_route_from_results(
                    &Err(RunAgentTurnError::Other {
                        phase: AgentTurnSubPhase::Executor,
                        message: "x".into(),
                    }),
                    &None,
                    false,
                    true,
                    false,
                ),
                StepIterationReduceAction::ExecOrVerifyFailed,
            ),
            (
                resolve_staged_step_post_outer_route_from_results(&ok, &None, true, true, false),
                StepIterationReduceAction::Cancelled,
            ),
            (
                resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, true),
                StepIterationReduceAction::ToolFailurePatch,
            ),
            (
                resolve_staged_step_post_outer_route_from_results(&ok, &None, false, true, false),
                StepIterationReduceAction::EmitSuccessAdvance,
            ),
        ];
        for (route, expect) in cases {
            assert_eq!(reduce_staged_step_post_outer_route(route), expect);
        }
    }
}
