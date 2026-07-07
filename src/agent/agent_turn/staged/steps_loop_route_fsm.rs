//! `staged_step_run_after_outer_half` 在 **transition 跳转** 之后的纯路由表
//!（`docs/design/per_state_machine_consolidation.md` §3.2 `StepRunning.sub`）。
//! **不**运行 outer_loop / 补丁 LLM / 不发 SSE。

use super::step_iteration_fsm::{
    StagedStepAfterOuterLoop, StagedStepToolPhaseRoute, staged_step_after_outer_loop,
    staged_step_tool_phase_route,
};
use crate::agent::agent_turn::errors::RunAgentTurnError;

/// transition 已排除后，本步 outer_loop 结果 → 下一步 I/O 形态（表驱动入口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedStepPostOuterRoute {
    /// 执行子循环 `Err` 或步级验收失败 → 补丁恢复或耗尽。
    ExecOrVerifyFailed,
    /// SSE 关闭或用户取消（outer 已成功）。
    Cancelled,
    /// 工具消息未全部成功且启用 patch planner。
    ToolFailurePatch,
    /// 本步成功收尾（含 patch 关闭时 tools 未全 ok 的既有语义）。
    EmitSuccess,
}

crate::impl_as_str!(StagedStepPostOuterRoute, {
    Self::ExecOrVerifyFailed => "exec_or_verify_failed",
    Self::Cancelled => "cancelled",
    Self::ToolFailurePatch => "tool_failure_patch",
    Self::EmitSuccess => "emit_success",
});

/// 由已分类的 **`StagedStepAfterOuterLoop`** 与取消/工具检查输入解析路由。
pub(crate) fn resolve_staged_step_post_outer_route(
    after_outer: StagedStepAfterOuterLoop,
    cancelled: bool,
    tools_ok: bool,
    patch_planner_on: bool,
) -> StagedStepPostOuterRoute {
    if matches!(
        after_outer,
        StagedStepAfterOuterLoop::ExecutionOrVerifyFailed { .. }
    ) {
        return StagedStepPostOuterRoute::ExecOrVerifyFailed;
    }
    if cancelled {
        return StagedStepPostOuterRoute::Cancelled;
    }
    match staged_step_tool_phase_route(tools_ok, patch_planner_on) {
        StagedStepToolPhaseRoute::AttemptToolFailurePatches => {
            StagedStepPostOuterRoute::ToolFailurePatch
        }
        StagedStepToolPhaseRoute::EmitStepSuccess => StagedStepPostOuterRoute::EmitSuccess,
    }
}

/// 组合 outer_loop 结果、验收与 cancel/tools 输入（供 **`steps_loop`** 单点调用）。
pub(crate) fn resolve_staged_step_post_outer_route_from_results(
    run_step: &Result<(), RunAgentTurnError>,
    step_verify_failed_reason: &Option<String>,
    cancelled: bool,
    tools_ok: bool,
    patch_planner_on: bool,
) -> StagedStepPostOuterRoute {
    let after_outer = staged_step_after_outer_loop(run_step, step_verify_failed_reason);
    resolve_staged_step_post_outer_route(after_outer, cancelled, tools_ok, patch_planner_on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};

    #[test]
    fn route_table_matches_legacy_branches() {
        let ok = Ok(());
        let err = Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "x".into(),
        });
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(&err, &None, false, true, false),
            StagedStepPostOuterRoute::ExecOrVerifyFailed
        );
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(
                &ok,
                &Some("vf".into()),
                false,
                true,
                false
            ),
            StagedStepPostOuterRoute::ExecOrVerifyFailed
        );
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(&ok, &None, true, true, false),
            StagedStepPostOuterRoute::Cancelled
        );
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, true),
            StagedStepPostOuterRoute::ToolFailurePatch
        );
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, false),
            StagedStepPostOuterRoute::EmitSuccess
        );
        assert_eq!(
            resolve_staged_step_post_outer_route_from_results(&ok, &None, false, true, false),
            StagedStepPostOuterRoute::EmitSuccess
        );
    }
}
