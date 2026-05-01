//! `run_staged_plan_with_prepared_request` 内 **首轮规划 assistant 解析** 的显式路由：
//! **`resolve_parse_with_assistant`** 将 **`Result<AgentReplyPlanV1, PlanArtifactError>`** 与
//! **`entered_from_step_execution_round`** 映射为 **`PreparedPlannerParseOutcome`**（纯函数，无 IO）。
//!
//! ensemble / 优化轮 **是否调用** 仍由 **`planner_round_fsm`** + **`post_parse_pipeline_fsm`** 计算；
//! 本模块仅收拢解析分支，避免 `mod.rs` 深层嵌套 `match`。
//!
//! 见 `docs/design/per_state_machine_consolidation.md`（分阶段回合编排）。

use crate::agent::plan_artifact::{AgentReplyPlanV1, PlanArtifactError};
use crate::types::Message;

use super::planner_parse_fsm::{StagedPlannerParseRoute, staged_planner_parse_route};

/// 解析一步的终端路由（调用方执行 IO：`push` / `outer_loop` / 继续后续 pipeline）。
#[derive(Debug)]
pub(crate) enum PreparedPlannerParseOutcome {
    /// 已得 **`AgentReplyPlanV1`**，进入 **no_task / ensemble / 优化 / 步循环**。
    ContinueWithPlan { plan: AgentReplyPlanV1 },
    /// **`QuietFinishOnPlanNotFound`**：本分阶段回合静默结束。
    QuietFinish,
    /// 降级到常规 **`run_agent_outer_loop`**；调用方使用外层已持有的 **`msg`** 写入历史。
    DegradeToOuterLoop,
}

/// 表驱动：对等旧实现中 **`parse_result` + `staged_planner_parse_route`** 分支。
pub(crate) fn resolve_parse_with_assistant(
    parse_result: Result<AgentReplyPlanV1, PlanArtifactError>,
    entered_from_step_execution_round: bool,
    _assistant_msg_for_api_compat: Message,
) -> PreparedPlannerParseOutcome {
    match parse_result {
        Ok(plan) => PreparedPlannerParseOutcome::ContinueWithPlan { plan },
        Err(parse_err) => {
            match staged_planner_parse_route(&parse_err, entered_from_step_execution_round) {
                StagedPlannerParseRoute::QuietFinishOnPlanNotFound => {
                    PreparedPlannerParseOutcome::QuietFinish
                }
                StagedPlannerParseRoute::DegradeToOuterLoop => {
                    PreparedPlannerParseOutcome::DegradeToOuterLoop
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;

    fn minimal_plan() -> AgentReplyPlanV1 {
        AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "s1".to_string(),
                description: "x".to_string(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            }],
            no_task: false,
        }
    }

    #[test]
    fn ok_parse_continues() {
        let plan = minimal_plan();
        let o = resolve_parse_with_assistant(Ok(plan.clone()), false, Message::user_only("u"));
        match o {
            PreparedPlannerParseOutcome::ContinueWithPlan { plan: p } => {
                assert_eq!(p.steps.len(), plan.steps.len());
            }
            _ => panic!("expected ContinueWithPlan"),
        }
    }

    #[test]
    fn not_found_entered_finishes_quiet() {
        let o = resolve_parse_with_assistant(
            Err(PlanArtifactError::NotFound),
            true,
            Message::user_only("u"),
        );
        assert!(matches!(o, PreparedPlannerParseOutcome::QuietFinish));
    }

    #[test]
    fn not_found_not_entered_degrades() {
        let o = resolve_parse_with_assistant(
            Err(PlanArtifactError::NotFound),
            false,
            Message::user_only("x"),
        );
        assert!(matches!(o, PreparedPlannerParseOutcome::DegradeToOuterLoop));
    }
}
