//! 分阶段 **`run_staged_plan_steps_loop`** 单次迭代在 **transition 已处理之后** 的纯决策：
//! outer_loop + 验收结果如何归类、工具健康检查阶段走哪条路径；以及墙钟是否超限（与循环顶部一致）。
//! **`StagedStepRunningSub`** 与 **`docs/design/per_state_machine_consolidation.md`** 中 `StepRunning.sub` 对齐（命名略宽：`AfterOuterLoop` 含成功收尾与失败补丁）。
//! **不**运行 outer_loop / 补丁 LLM / 不发 SSE。

use crate::agent::agent_turn::errors::RunAgentTurnError;

/// 单步执行器内子阶段（对应设计稿 **`StepRunning.sub`**：`BeforeStepLlm` / `InOuterLoop` / 失败处理子集）。
/// 实现上由 **`staged/mod.rs`** 的 **`staged_step_run_outer_half`** / **`staged_step_run_after_outer_half`** 对应；本类型为**词汇表**（检索/文档对齐），生产路径不直接分支于该枚举。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepRunningSub {
    /// `step_started` 起至注入本步 user、设置 `turn_planner_hints.step_executor_constraint` 止（尚未 `run_agent_outer_loop`）。
    BeforeStepLlm,
    /// `run_agent_outer_loop` 与可选 acceptance 验证。
    InOuterLoop,
    /// outer 返回之后：transition、执行/验收失败补丁、取消、工具消息检查与补丁、或成功 SSE（设计稿中的 *AfterStepFailure* 为该阶段内子路径）。
    AfterOuterLoop,
}

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

/// 单次 `run_staged_plan_steps_loop` 迭代结束方式（不含墙钟：由外层检查）。
pub(crate) enum StagedStepIterationCtl {
    /// 补丁重规划后重试当前下标（`i` 不变）。
    RetryCurrentStep { n: usize },
    /// 本步已完结（transition 或成功），调用方将 `i += 1`。
    AdvanceToNextStep { n: usize, completed_steps: usize },
    /// 本步成功后检测到取消（与历史：先发 `step_finished(cancelled)` 再 `break`）。
    CancelledAfterOuterOk,
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

/// 与 [`crate::agent::turn_budget::turn_wall_clock_exceeded`] 一致：`max_turn_duration_seconds == 0` 表示不限制。
pub(crate) fn staged_step_wall_clock_exceeded(
    max_turn_duration_seconds: u64,
    elapsed_secs: u64,
) -> bool {
    crate::agent::turn_budget::turn_wall_clock_exceeded(max_turn_duration_seconds, elapsed_secs)
}

pub(crate) fn staged_step_verify_fail_patch_detail(verify_reason: &str) -> String {
    format!(
        "验证闸门报告失败: {}\n请根据对话历史缩短或调整后续步骤，并在补丁中修复此问题。",
        verify_reason
    )
}

pub(crate) const STAGED_STEP_OUTER_LOOP_FAIL_DETAIL: &str =
    "请根据对话历史缩短或调整后续步骤；若属环境/权限问题请在补丁中显式增加修复步。";

pub(crate) const STAGED_STEP_TOOL_MSG_FAIL_DETAIL: &str = "请阅读本步对应的 `role: tool` 输出（含失败原因），修订从当前步起的 `steps`（可替换、拆分或追加一步）。";

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

    #[test]
    fn wall_clock_exceeded_matches_loop() {
        assert!(!staged_step_wall_clock_exceeded(0, 999));
        assert!(!staged_step_wall_clock_exceeded(10, 10));
        assert!(staged_step_wall_clock_exceeded(10, 11));
    }
}
