//! 分阶段规划 **回合级** 状态机：单次「规划→执行」子调用结束后，决定下一轮相位与副作用。
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2；步内 `outer_loop` 不在此模块表达。

use crate::types::Message;

use super::super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::StagedPlanRunOutcome;

/// 分阶段单用户回合外层循环的可变相位（滚动视界：步完成后再次进入规划）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedTurnPhase {
    /// 尚未在本回合内完成过「步执行→ContinuePlanning」；首轮规划或全局重规划后。
    PreStepExecutionRound,
    /// 至少完成过一异步阶段规划队列的一步，`entered_from_step_execution_round == true`。
    AfterStepExecutionRound,
}

/// 进入 [`advance_staged_turn_after_sub_call`] 的事件：一次 `run_staged_plan_with_prepared_request` 已返回。
#[derive(Debug)]
pub(crate) enum StagedTurnSubCallOutcome {
    Ok(StagedPlanRunOutcome),
    Err(RunAgentTurnError),
}

/// 推进后的回合路由（驱动层据此执行 IO：`continue` / `return Ok` / `return Err`）。
#[derive(Debug)]
pub(crate) enum StagedTurnAdvance {
    /// 继续外层循环；可选追加全局重规划 user（`StepRetryExhausted` 回收路径）。
    Continue {
        phase: StagedTurnPhase,
        push_feedback_user: Option<Message>,
    },
    /// 本分阶段回合正常结束。
    Finished,
    /// 全局重规划次数用尽。
    ReplanExhausted {
        phase: AgentTurnSubPhase,
        message: String,
    },
    /// 其它错误原样向上传递。
    Propagate(RunAgentTurnError),
}

impl StagedTurnPhase {
    pub(crate) fn entered_from_step_execution_round(self) -> bool {
        matches!(self, Self::AfterStepExecutionRound)
    }
}

/// 由当前相位与一次子调用结果推导下一相位与副作用（**无** IO、**无**快照回滚）。
pub(crate) fn advance_staged_turn_after_sub_call(
    _phase: StagedTurnPhase,
    rewrite_attempts: usize,
    max_rewrites: usize,
    event: StagedTurnSubCallOutcome,
) -> StagedTurnAdvance {
    match event {
        StagedTurnSubCallOutcome::Ok(StagedPlanRunOutcome::ContinuePlanning) => {
            StagedTurnAdvance::Continue {
                phase: StagedTurnPhase::AfterStepExecutionRound,
                push_feedback_user: None,
            }
        }
        StagedTurnSubCallOutcome::Ok(StagedPlanRunOutcome::Finished) => StagedTurnAdvance::Finished,
        StagedTurnSubCallOutcome::Err(RunAgentTurnError::StepRetryExhausted {
            phase: sub,
            message,
        }) if rewrite_attempts >= max_rewrites => StagedTurnAdvance::ReplanExhausted {
            phase: sub,
            message: format!(
                "全局重规划耗尽上限 ({} 次)。最后失败原因: {}",
                max_rewrites, message
            ),
        },
        StagedTurnSubCallOutcome::Err(RunAgentTurnError::StepRetryExhausted {
            message, ..
        }) => {
            let fb = format!(
                "### 全局重规划要求\n\
                 由于前面的步骤执行多次修复后仍然彻底失败，失败原因摘要如下：\n\
                 {}\n\n\
                 请你结合上述失败经验，抛弃之前的旧计划，重新思考并给出一份**全新的**分阶段规划（agent_reply_plan v1）。\n\
                 我们将从新计划的第一步重新开始执行。",
                message
            );
            StagedTurnAdvance::Continue {
                phase: StagedTurnPhase::PreStepExecutionRound,
                push_feedback_user: Some(Message::user_only(fb)),
            }
        }
        StagedTurnSubCallOutcome::Err(e) => StagedTurnAdvance::Propagate(e),
    }
}

/// 与 [`advance_staged_turn_after_sub_call`] 配套的计数递增：仅在进入全局重规划反馈路径时 `+1`。
pub(crate) fn next_rewrite_attempts_after_advance(
    prev: usize,
    advance: &StagedTurnAdvance,
) -> usize {
    match advance {
        StagedTurnAdvance::Continue {
            push_feedback_user: Some(_),
            ..
        } => prev.saturating_add(1),
        _ => prev,
    }
}

/// `entered_from_step_execution_round` 传给下一轮子调用（对应原布尔）。
pub(crate) fn entered_flag_for_next_planner_call(phase: StagedTurnPhase) -> bool {
    phase.entered_from_step_execution_round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continue_planning_moves_to_after_step() {
        let a = advance_staged_turn_after_sub_call(
            StagedTurnPhase::PreStepExecutionRound,
            0,
            2,
            StagedTurnSubCallOutcome::Ok(StagedPlanRunOutcome::ContinuePlanning),
        );
        match a {
            StagedTurnAdvance::Continue {
                phase: StagedTurnPhase::AfterStepExecutionRound,
                push_feedback_user: None,
            } => {}
            other => panic!("unexpected advance: {:?}", other),
        }
    }

    #[test]
    fn finished_stops_loop() {
        assert!(matches!(
            advance_staged_turn_after_sub_call(
                StagedTurnPhase::AfterStepExecutionRound,
                0,
                2,
                StagedTurnSubCallOutcome::Ok(StagedPlanRunOutcome::Finished),
            ),
            StagedTurnAdvance::Finished
        ));
    }

    #[test]
    fn step_retry_exhausted_pushes_feedback_until_cap() {
        let adv = advance_staged_turn_after_sub_call(
            StagedTurnPhase::AfterStepExecutionRound,
            0,
            2,
            StagedTurnSubCallOutcome::Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: "boom".to_string(),
            }),
        );
        match adv {
            StagedTurnAdvance::Continue {
                phase: StagedTurnPhase::PreStepExecutionRound,
                push_feedback_user: Some(ref m),
            } => {
                assert!(
                    crate::types::message_content_as_str(&m.content)
                        .is_some_and(|s| s.contains("boom"))
                );
            }
            other => panic!("unexpected {:?}", other),
        }
        assert_eq!(next_rewrite_attempts_after_advance(0, &adv), 1);
    }

    #[test]
    fn step_retry_exhausted_at_cap_returns_replan_exhausted() {
        let adv = advance_staged_turn_after_sub_call(
            StagedTurnPhase::AfterStepExecutionRound,
            2,
            2,
            StagedTurnSubCallOutcome::Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Planner,
                message: "last".to_string(),
            }),
        );
        match adv {
            StagedTurnAdvance::ReplanExhausted { message, .. } => {
                assert!(message.contains("耗尽上限"));
                assert!(message.contains("last"));
            }
            other => panic!("unexpected {:?}", other),
        }
    }
}
