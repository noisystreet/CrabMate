//! 滚动视界 **`StagedTurnAdvance`** → 无 IO 的 reduce（表驱动；IO 仍在 **`rolling_horizon_facade`**）。

use super::turn_fsm::{StagedTurnAdvance, StagedTurnPhase};

/// `staged_rolling_horizon_apply_advance` 之后的纯 reduce 输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollingHorizonAdvanceReduceAction {
    ContinueLoop {
        next_phase: StagedTurnPhase,
        restore_workspace_snapshot: bool,
        push_feedback_user: bool,
    },
    Finish,
    ReplanExhausted,
    Propagate,
}

impl RollingHorizonAdvanceReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ContinueLoop { .. } => "continue_loop",
            Self::Finish => "finish",
            Self::ReplanExhausted => "replan_exhausted",
            Self::Propagate => "propagate",
        }
    }
}

pub(crate) fn reduce_rolling_horizon_advance(
    advance: &StagedTurnAdvance,
) -> RollingHorizonAdvanceReduceAction {
    match advance {
        StagedTurnAdvance::Continue {
            phase,
            push_feedback_user,
        } => RollingHorizonAdvanceReduceAction::ContinueLoop {
            next_phase: *phase,
            restore_workspace_snapshot: push_feedback_user.is_some(),
            push_feedback_user: push_feedback_user.is_some(),
        },
        StagedTurnAdvance::Finished => RollingHorizonAdvanceReduceAction::Finish,
        StagedTurnAdvance::ReplanExhausted { .. } => {
            RollingHorizonAdvanceReduceAction::ReplanExhausted
        }
        StagedTurnAdvance::Propagate(_) => RollingHorizonAdvanceReduceAction::Propagate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};
    use crate::types::Message;

    #[test]
    fn reduce_continue_without_feedback() {
        let action = reduce_rolling_horizon_advance(&StagedTurnAdvance::Continue {
            phase: StagedTurnPhase::AfterStepExecutionRound,
            push_feedback_user: None,
        });
        assert_eq!(
            action,
            RollingHorizonAdvanceReduceAction::ContinueLoop {
                next_phase: StagedTurnPhase::AfterStepExecutionRound,
                restore_workspace_snapshot: false,
                push_feedback_user: false,
            }
        );
    }

    #[test]
    fn reduce_continue_with_global_replan_feedback() {
        let action = reduce_rolling_horizon_advance(&StagedTurnAdvance::Continue {
            phase: StagedTurnPhase::PreStepExecutionRound,
            push_feedback_user: Some(Message::user_only("fb")),
        });
        assert_eq!(
            action,
            RollingHorizonAdvanceReduceAction::ContinueLoop {
                next_phase: StagedTurnPhase::PreStepExecutionRound,
                restore_workspace_snapshot: true,
                push_feedback_user: true,
            }
        );
    }

    #[test]
    fn reduce_finished_and_propagate() {
        assert_eq!(
            reduce_rolling_horizon_advance(&StagedTurnAdvance::Finished),
            RollingHorizonAdvanceReduceAction::Finish
        );
        assert_eq!(
            reduce_rolling_horizon_advance(&StagedTurnAdvance::Propagate(
                RunAgentTurnError::Other {
                    phase: AgentTurnSubPhase::Planner,
                    message: "x".into(),
                }
            )),
            RollingHorizonAdvanceReduceAction::Propagate
        );
    }
}
