//! 分阶段 **`run_staged_plan_steps_loop`** 单次迭代在 **transition 已处理之后** 的纯决策：
//! outer_loop + 验收结果如何归类、工具健康检查阶段走哪条路径。
//! **不**运行 outer_loop / 补丁 LLM / 不发 SSE。

use crate::agent::agent_turn::errors::RunAgentTurnError;

/// `try_apply_staged_plan_control_flow_jump` 未触发时，根据 outer_loop 与验收结果划分阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepAfterOuterLoop {
    /// 执行与验收均成功，进入「本步 user 之后 tool 消息是否均 ok」的检查。
    ProceedToToolCheck,
    /// 执行失败或验收失败；由调用方跑补丁循环或早退。
    ExecutionOrVerifyFailed {
        outer_loop_error: Option<String>,
        verify_failure_reason: Option<String>,
    },
}

pub(crate) fn staged_step_after_outer_loop(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> StagedStepAfterOuterLoop {
    if let Err(e) = run_step {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: Some(e.to_string()),
            verify_failure_reason: None,
        };
    }
    if let Some(r) = step_verify_failed_reason {
        return StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: None,
            verify_failure_reason: Some(r.clone()),
        };
    }
    StagedStepAfterOuterLoop::ProceedToToolCheck
}

/// 失败路径上补丁耗尽时构造 `StepRetryExhausted` 文案（与历史 `run_staged_plan_steps_loop` 一致）。
pub(crate) fn staged_step_failure_retry_exhausted_message(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
) -> String {
    if let Err(e) = run_step {
        return e.to_string();
    }
    step_verify_failed_reason
        .clone()
        .unwrap_or_else(|| "局部修复耗尽上限".to_string())
}

/// 工具消息检查阶段：是否进入「工具未全部成功」的补丁尝试循环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepToolPhaseRoute {
    /// 发送本步 `ok` 并推进（含 `tools_ok==false` 且未启用 patch planner 时沿用既有语义）。
    EmitStepSuccess,
    /// `tools_ok==false` 且启用 patch planner：由调用方跑补丁循环，可能 `continue` 同一步。
    AttemptToolFailurePatches,
}

pub(crate) fn staged_step_tool_phase_route(
    tools_ok: bool,
    patch_planner_enabled: bool,
) -> StagedStepToolPhaseRoute {
    if tools_ok {
        StagedStepToolPhaseRoute::EmitStepSuccess
    } else if patch_planner_enabled {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches
    } else {
        StagedStepToolPhaseRoute::EmitStepSuccess
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};

    #[test]
    fn after_outer_loop_err_skips_verify() {
        let err = Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "x".into(),
        });
        let r = staged_step_after_outer_loop(&err, &Some("verify".into()));
        assert_eq!(
            r,
            StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
                outer_loop_error: Some("x".into()),
                verify_failure_reason: None,
            }
        );
    }

    #[test]
    fn after_outer_loop_ok_and_verify_fail() {
        let ok = Ok(());
        let r = staged_step_after_outer_loop(&ok, &Some("bad".into()));
        assert_eq!(
            r,
            StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
                outer_loop_error: None,
                verify_failure_reason: Some("bad".into()),
            }
        );
    }

    #[test]
    fn after_outer_loop_proceed() {
        let ok = Ok(());
        assert_eq!(
            staged_step_after_outer_loop(&ok, &None),
            StagedStepAfterOuterLoop::ProceedToToolCheck
        );
    }

    #[test]
    fn exhausted_message_prefers_outer_err() {
        let err = Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "oe".into(),
        });
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&err, &Some("v".into())),
            "oe"
        );
    }

    #[test]
    fn exhausted_message_verify_or_default() {
        let ok = Ok(());
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&ok, &Some("vf".into())),
            "vf"
        );
        assert_eq!(
            staged_step_failure_retry_exhausted_message(&ok, &None),
            "局部修复耗尽上限"
        );
    }

    #[test]
    fn tool_phase_routes() {
        assert_eq!(
            staged_step_tool_phase_route(true, false),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(true, true),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(false, false),
            StagedStepToolPhaseRoute::EmitStepSuccess
        );
        assert_eq!(
            staged_step_tool_phase_route(false, true),
            StagedStepToolPhaseRoute::AttemptToolFailurePatches
        );
    }
}
