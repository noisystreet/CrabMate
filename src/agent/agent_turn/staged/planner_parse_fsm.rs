//! 分阶段规划首轮：`agent_reply_plan` v1 解析结果与 `no_task` 写入历史的纯路由。
//! 与 `run_staged_plan_with_prepared_request` 对齐；与 **`planner_round_fsm`**（ensemble / 优化轮）正交；**不**发起 LLM。

use crate::agent::plan_artifact::PlanArtifactError;

/// 首轮规划 assistant 解析失败时的上层路由（相对「本轮 turn」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlannerParseRoute {
    /// `NotFound` 且由步执行回灌触发：视为滚动规划收敛，静默结束本分阶段回合。
    QuietFinishOnPlanNotFound,
    /// 其它解析错误或首轮「未找到结构化规划」：保留正文并降级到常规 `outer_loop`。
    DegradeToOuterLoop,
}

/// `entered_from_step_execution_round == true` 时，`NotFound` 收敛结束；否则降级。
pub(crate) fn staged_planner_parse_route(
    err: &PlanArtifactError,
    entered_from_step_execution_round: bool,
) -> StagedPlannerParseRoute {
    if matches!(err, PlanArtifactError::NotFound)
        && entered_implies_finish_on_plan_not_found(entered_from_step_execution_round)
    {
        StagedPlannerParseRoute::QuietFinishOnPlanNotFound
    } else {
        StagedPlannerParseRoute::DegradeToOuterLoop
    }
}

/// Web 且未开启 RAW：对 `no_task` 规划不向会话写入 assistant（由 NL 轮承担可见输出）。
#[inline]
pub(crate) fn omit_no_task_planner_from_history(
    web_out_active: bool,
    web_raw_assistant_output: bool,
    plan_no_task: bool,
) -> bool {
    web_out_active && !web_raw_assistant_output && plan_no_task
}

/// 与「步执行后重入的无工具规划轮」标记对齐：`true` 时 `NotFound` 走静默收敛（见 [`staged_planner_parse_route`]）。
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
    fn not_found_finishes_only_when_entered_from_step_round() {
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, false),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, true),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        );
    }

    #[test]
    fn non_not_found_always_degrades() {
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::WrongType("x".into()), true),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::EmptySteps, true),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
    }

    #[test]
    fn omit_no_task_only_on_web_without_raw() {
        assert!(omit_no_task_planner_from_history(true, false, true));
        assert!(!omit_no_task_planner_from_history(false, false, true));
        assert!(!omit_no_task_planner_from_history(true, true, true));
        assert!(!omit_no_task_planner_from_history(true, false, false));
    }

    #[test]
    fn entered_flag_matches_not_found_route() {
        assert!(!matches!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, false),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        ));
        assert!(matches!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, true),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        ));
    }
}
