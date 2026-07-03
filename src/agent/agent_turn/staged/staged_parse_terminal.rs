//! 分阶段首轮解析 **终端路由**（两层化：`StagedPlannerParseRoute` → 本枚举 → 驱动/观测）。
//! 见 `docs/design/per_state_machine_consolidation.md` §3.2 与 **`prepared_parse_fsm::PreparedPlannerRoute`**。

use crate::agent::plan_artifact::AgentReplyPlanV1;

use super::planner_parse_fsm::StagedPlannerParseRoute;
use super::prepared_parse_fsm::PreparedPlannerRoute;

/// 规划 assistant 解析后的**终端**路由（不含 post-parse 子管线细节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedParseTerminalRoute {
    QuietFinish,
    DegradeToOuterLoop,
    FinishWithDirectAnswer,
    ContinueWithPlan { plan: AgentReplyPlanV1 },
}

impl StagedParseTerminalRoute {
    pub(crate) fn as_static_str(&self) -> &'static str {
        match self {
            Self::QuietFinish => "quiet_finish",
            Self::DegradeToOuterLoop => "degrade_to_outer_loop",
            Self::FinishWithDirectAnswer => "finish_with_direct_planner_answer",
            Self::ContinueWithPlan { .. } => "continue_with_plan",
        }
    }

    pub(crate) fn from_planner_parse_route(route: StagedPlannerParseRoute) -> Self {
        match route {
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound => Self::QuietFinish,
            StagedPlannerParseRoute::FinishOnDirectPlannerAnswer => Self::FinishWithDirectAnswer,
            StagedPlannerParseRoute::DegradeToOuterLoop => Self::DegradeToOuterLoop,
        }
    }

    pub(crate) fn from_prepared_planner_route(route: &PreparedPlannerRoute) -> Self {
        match route {
            PreparedPlannerRoute::QuietFinish => Self::QuietFinish,
            PreparedPlannerRoute::DegradeToOuterLoop => Self::DegradeToOuterLoop,
            PreparedPlannerRoute::FinishWithDirectPlannerAnswer => Self::FinishWithDirectAnswer,
            PreparedPlannerRoute::ContinueWithPlan { plan } => {
                Self::ContinueWithPlan { plan: plan.clone() }
            }
        }
    }

    pub(crate) fn to_prepared_planner_route(&self) -> PreparedPlannerRoute {
        match self {
            Self::QuietFinish => PreparedPlannerRoute::QuietFinish,
            Self::DegradeToOuterLoop => PreparedPlannerRoute::DegradeToOuterLoop,
            Self::FinishWithDirectAnswer => PreparedPlannerRoute::FinishWithDirectPlannerAnswer,
            Self::ContinueWithPlan { plan } => {
                PreparedPlannerRoute::ContinueWithPlan { plan: plan.clone() }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::AgentReplyPlanV1;

    #[test]
    fn terminal_route_roundtrip_with_prepared() {
        let plan = AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".into(),
            version: 1,
            steps: vec![],
            no_task: true,
        };
        let prepared = PreparedPlannerRoute::ContinueWithPlan { plan: plan.clone() };
        let terminal = StagedParseTerminalRoute::from_prepared_planner_route(&prepared);
        assert_eq!(terminal.as_static_str(), "continue_with_plan");
        match terminal.to_prepared_planner_route() {
            PreparedPlannerRoute::ContinueWithPlan { plan: p } => assert_eq!(p, plan),
            _ => panic!("expected continue_with_plan"),
        }
    }

    #[test]
    fn planner_parse_maps_to_terminal() {
        assert_eq!(
            StagedParseTerminalRoute::from_planner_parse_route(
                StagedPlannerParseRoute::QuietFinishOnPlanNotFound
            )
            .as_static_str(),
            "quiet_finish"
        );
    }
}
