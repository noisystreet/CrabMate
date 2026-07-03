//! 首轮 **`PreparedPlannerRoute`** → 无 IO 的 reduce 动作（表驱动；IO 仍在 **`mod.rs`**）。

use super::prepared_parse_fsm::PreparedPlannerRoute;

/// `resolve_prepared_planner_route` 之后的纯 reduce 输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreparedRouteReduceAction {
    /// 分步后重入且未产出计划：静默结束。
    FinishQuiet,
    /// 只读概览类终答：落盘 assistant 后结束。
    FinishWithAssistantOnly,
    /// 解析失败等：落盘 assistant 后走外循环。
    DegradeToOuterLoop,
    /// 合法 `agent_reply_plan`：进入 post-parse 管线（no_task / full-pipeline）。
    ContinuePostParse,
}

impl PreparedRouteReduceAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FinishQuiet => "finish_quiet",
            Self::FinishWithAssistantOnly => "finish_with_assistant_only",
            Self::DegradeToOuterLoop => "degrade_to_outer_loop",
            Self::ContinuePostParse => "continue_post_parse",
        }
    }
}

pub(crate) fn reduce_prepared_planner_route(
    route: &PreparedPlannerRoute,
) -> PreparedRouteReduceAction {
    match route {
        PreparedPlannerRoute::QuietFinish => PreparedRouteReduceAction::FinishQuiet,
        PreparedPlannerRoute::FinishWithDirectPlannerAnswer => {
            PreparedRouteReduceAction::FinishWithAssistantOnly
        }
        PreparedPlannerRoute::DegradeToOuterLoop => PreparedRouteReduceAction::DegradeToOuterLoop,
        PreparedPlannerRoute::ContinueWithPlan { .. } => {
            PreparedRouteReduceAction::ContinuePostParse
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::AgentReplyPlanV1;

    #[test]
    fn reduce_matches_prepared_route_variants() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![],
            no_task: false,
        };
        assert_eq!(
            reduce_prepared_planner_route(&PreparedPlannerRoute::QuietFinish),
            PreparedRouteReduceAction::FinishQuiet
        );
        assert_eq!(
            reduce_prepared_planner_route(&PreparedPlannerRoute::FinishWithDirectPlannerAnswer),
            PreparedRouteReduceAction::FinishWithAssistantOnly
        );
        assert_eq!(
            reduce_prepared_planner_route(&PreparedPlannerRoute::DegradeToOuterLoop),
            PreparedRouteReduceAction::DegradeToOuterLoop
        );
        assert_eq!(
            reduce_prepared_planner_route(&PreparedPlannerRoute::ContinueWithPlan { plan }),
            PreparedRouteReduceAction::ContinuePostParse
        );
    }
}
