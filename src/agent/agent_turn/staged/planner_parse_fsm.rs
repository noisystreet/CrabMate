//! 分阶段规划轮 **assistant 正文解析 `agent_reply_plan` v1** 之后的纯路由（无 IO）。
//! 与 `planner_round_fsm`（ensemble / 优化轮）正交：本模块处理 **解析失败** 与 **`no_task` 历史省略** 判定。

use crate::agent::plan_artifact::PlanArtifactError;

/// `parse_agent_reply_plan_v1_*` 失败后的编排路由（驱动层决定是否直接结束或降级 `outer_loop`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlannerParseRoute {
    /// `NotFound` 且已进入步后重规划轮：静默结束本分阶段回合（不发降级日志）。
    FinishTurnQuietly,
    /// 保留 assistant、降级常规工具循环。
    DegradeToOuterLoop,
}

/// 与原先 `if NotFound && should_finish_when_plan_not_found` / `else 降级` 等价。
pub(crate) fn staged_planner_parse_route(
    err: &PlanArtifactError,
    entered_from_step_execution_round: bool,
) -> StagedPlannerParseRoute {
    if matches!(err, PlanArtifactError::NotFound)
        && entered_implies_finish_on_plan_not_found(entered_from_step_execution_round)
    {
        StagedPlannerParseRoute::FinishTurnQuietly
    } else {
        StagedPlannerParseRoute::DegradeToOuterLoop
    }
}

/// Web 路径且非 RAW 展示、`no_task=true` 时不将本轮规划 assistant 写入会话（与 `run_staged_plan_with_prepared_request` 一致）。
#[inline]
pub(crate) fn omit_no_task_planner_from_history(
    web_sse_out_is_some: bool,
    web_raw_assistant_output_env: bool,
    plan_no_task: bool,
) -> bool {
    web_sse_out_is_some && !web_raw_assistant_output_env && plan_no_task
}

/// 步后重入且无结构化规划时是否直接收敛（原 `should_finish_when_plan_not_found`）。
#[inline]
pub(crate) fn entered_implies_finish_on_plan_not_found(
    entered_from_step_execution_round: bool,
) -> bool {
    entered_from_step_execution_round
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_after_step_finishes_quietly() {
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, true),
            StagedPlannerParseRoute::FinishTurnQuietly
        );
    }

    #[test]
    fn not_found_first_round_degrades() {
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, false),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
    }

    #[test]
    fn invalid_plan_always_degrades() {
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::EmptySteps, true),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
    }

    #[test]
    fn omit_no_task_only_when_web_non_raw() {
        assert!(omit_no_task_planner_from_history(true, false, true));
        assert!(!omit_no_task_planner_from_history(true, true, true));
        assert!(!omit_no_task_planner_from_history(false, false, true));
    }
}
