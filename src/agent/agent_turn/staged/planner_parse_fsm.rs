//! 分阶段规划首轮：`agent_reply_plan` v1 解析结果与 `no_task` 写入历史的纯路由。
//! 与 `run_staged_plan_with_prepared_request` 对齐；与 **`planner_round_fsm`**（ensemble / 优化轮）正交；**不**发起 LLM。

use crate::agent::plan_artifact::PlanArtifactError;

use super::super::intent::readonly_overview_bypass;

/// 规划轮 assistant 合并正文（思维链+正文）视为「已直接作答」的最小字符数（Unicode 标量）。
pub(crate) const PLANNER_DIRECT_ANSWER_MIN_CHARS: usize = 240;

/// 首轮规划 assistant 解析失败时的上层路由（相对「本轮 turn」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlannerParseRoute {
    /// `NotFound` 且由步执行回灌触发：视为滚动规划收敛，静默结束本分阶段回合。
    QuietFinishOnPlanNotFound,
    /// 首轮 `NotFound` 但已有实质 Markdown 终答且用户任务为只读概览类：落盘 assistant 后结束，**不**降级外循环。
    FinishOnDirectPlannerAnswer,
    /// 其它解析错误或首轮「未找到结构化规划」：保留正文并降级到常规 `outer_loop`。
    DegradeToOuterLoop,
}

/// `entered_from_step_execution_round == true` 时，`NotFound` 收敛结束；首轮只读概览可 [`FinishOnDirectPlannerAnswer`]；否则降级。
pub(crate) fn staged_planner_parse_route(
    err: &PlanArtifactError,
    entered_from_step_execution_round: bool,
    merged_answer_text: &str,
    user_task: Option<&str>,
) -> StagedPlannerParseRoute {
    if matches!(err, PlanArtifactError::NotFound) {
        if entered_implies_finish_on_plan_not_found(entered_from_step_execution_round) {
            return StagedPlannerParseRoute::QuietFinishOnPlanNotFound;
        }
        if should_finish_on_direct_planner_answer(merged_answer_text, user_task) {
            return StagedPlannerParseRoute::FinishOnDirectPlannerAnswer;
        }
    }
    StagedPlannerParseRoute::DegradeToOuterLoop
}

/// 首轮无 JSON 规划、但规划轮已写出足够长的只读分析类正文时，不再调用外循环。
#[inline]
pub(crate) fn should_finish_on_direct_planner_answer(
    merged_answer_text: &str,
    user_task: Option<&str>,
) -> bool {
    if !planner_answer_text_substantive_enough(merged_answer_text) {
        return false;
    }
    match user_task.map(str::trim).filter(|s| !s.is_empty()) {
        Some(task) => {
            readonly_overview_bypass::readonly_overview_task_heuristic(task)
                || task_looks_like_readonly_overview_short(task)
        }
        None => false,
    }
}

#[inline]
pub(crate) fn planner_answer_text_substantive_enough(merged_answer_text: &str) -> bool {
    merged_answer_text.chars().count() >= PLANNER_DIRECT_ANSWER_MIN_CHARS
}

/// 极短 user 句（无「分析」等 consult 词）时的补充匹配。
fn task_looks_like_readonly_overview_short(task: &str) -> bool {
    let lower = task.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    if super::super::intent::advisory_bypass::task_has_impl_strength_markers(&lower, &[]) {
        return false;
    }
    matches!(
        lower.as_str(),
        "分析当前项目"
            | "分析项目"
            | "分析仓库"
            | "分析代码库"
            | "介绍当前项目"
            | "项目概览"
            | "仓库概览"
    )
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

    fn long_overview_answer() -> String {
        "好的，我来分析当前项目。\n\n## 项目总览\n\n".repeat(20)
    }

    #[test]
    fn not_found_finishes_only_when_entered_from_step_round() {
        assert_eq!(
            staged_planner_parse_route(
                &PlanArtifactError::NotFound,
                false,
                "short",
                Some("分析当前项目"),
            ),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
        assert_eq!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, true, "short", None),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        );
    }

    #[test]
    fn not_found_substantive_readonly_overview_finishes_without_outer() {
        assert_eq!(
            staged_planner_parse_route(
                &PlanArtifactError::NotFound,
                false,
                long_overview_answer().as_str(),
                Some("分析当前项目"),
            ),
            StagedPlannerParseRoute::FinishOnDirectPlannerAnswer
        );
    }

    #[test]
    fn not_found_substantive_but_impl_task_still_degrades() {
        assert_eq!(
            staged_planner_parse_route(
                &PlanArtifactError::NotFound,
                false,
                long_overview_answer().as_str(),
                Some("分析当前项目并请修改 main.rs"),
            ),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
    }

    #[test]
    fn non_not_found_always_degrades() {
        assert_eq!(
            staged_planner_parse_route(
                &PlanArtifactError::WrongType("x".into()),
                true,
                long_overview_answer().as_str(),
                Some("分析当前项目"),
            ),
            StagedPlannerParseRoute::DegradeToOuterLoop
        );
        assert_eq!(
            staged_planner_parse_route(
                &PlanArtifactError::EmptySteps,
                false,
                long_overview_answer().as_str(),
                Some("分析当前项目"),
            ),
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
}
