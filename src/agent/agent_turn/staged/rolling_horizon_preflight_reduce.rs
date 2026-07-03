//! 滚动视界外层循环 **preflight** → 无 IO 的 reduce（表驱动；IO 仍在 **`rolling_horizon_facade`**）。

use super::turn_fsm::StagedTurnPhase;

/// preflight 判定入参（早停结论由调用方预先计算，避免本模块依赖 `RunLoopParams`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RollingHorizonPreflightInput {
    pub staged_rounds: usize,
    pub max_rounds: usize,
    pub phase: StagedTurnPhase,
    /// `AfterStepExecutionRound` 且任务级证据已满足时为 `true`。
    pub early_stop_allow: bool,
}

/// `staged_rolling_horizon_preflight_exit` 的纯 reduce 输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollingHorizonPreflightAction {
    ContinueIteration,
    StopMaxRounds,
    StopEarlyFinish,
}

impl RollingHorizonPreflightAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ContinueIteration => "continue_iteration",
            Self::StopMaxRounds => "stop_max_rounds",
            Self::StopEarlyFinish => "stop_early_finish",
        }
    }
}

pub(crate) fn reduce_rolling_horizon_preflight(
    input: RollingHorizonPreflightInput,
) -> RollingHorizonPreflightAction {
    if input.staged_rounds > input.max_rounds {
        return RollingHorizonPreflightAction::StopMaxRounds;
    }
    if matches!(input.phase, StagedTurnPhase::AfterStepExecutionRound) && input.early_stop_allow {
        return RollingHorizonPreflightAction::StopEarlyFinish;
    }
    RollingHorizonPreflightAction::ContinueIteration
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_continue_before_cap() {
        assert_eq!(
            reduce_rolling_horizon_preflight(RollingHorizonPreflightInput {
                staged_rounds: 2,
                max_rounds: 64,
                phase: StagedTurnPhase::PreStepExecutionRound,
                early_stop_allow: false,
            }),
            RollingHorizonPreflightAction::ContinueIteration
        );
    }

    #[test]
    fn preflight_stop_max_rounds() {
        assert_eq!(
            reduce_rolling_horizon_preflight(RollingHorizonPreflightInput {
                staged_rounds: 65,
                max_rounds: 64,
                phase: StagedTurnPhase::AfterStepExecutionRound,
                early_stop_allow: false,
            }),
            RollingHorizonPreflightAction::StopMaxRounds
        );
    }

    #[test]
    fn preflight_early_finish_after_step_round() {
        assert_eq!(
            reduce_rolling_horizon_preflight(RollingHorizonPreflightInput {
                staged_rounds: 3,
                max_rounds: 64,
                phase: StagedTurnPhase::AfterStepExecutionRound,
                early_stop_allow: true,
            }),
            RollingHorizonPreflightAction::StopEarlyFinish
        );
    }

    #[test]
    fn preflight_no_early_finish_on_pre_step_round() {
        assert_eq!(
            reduce_rolling_horizon_preflight(RollingHorizonPreflightInput {
                staged_rounds: 3,
                max_rounds: 64,
                phase: StagedTurnPhase::PreStepExecutionRound,
                early_stop_allow: true,
            }),
            RollingHorizonPreflightAction::ContinueIteration
        );
    }
}
