//! `run_staged_plan_with_prepared_request` 内 **首轮规划 assistant 解析** 的显式路由：
//! **`resolve_parse_with_assistant`** 将 **`Result<AgentReplyPlanV1, PlanArtifactError>`** 与
//! **`entered_from_step_execution_round`** 映射为 **`PreparedPlannerParseOutcome`**（纯函数，无 IO）。
//!
//! **`PreparedPlannerRoute`**：首轮解析后对 **`run_staged_plan_with_prepared_request`** 主路径的**终端路由**
//!（静默结束 / 降级 outer_loop / 进入 post-parse 管线）；与 **`PreparedPlannerParseOutcome`** 的关系见
//! **[`resolve_prepared_planner_route`]**。
//!
//! ensemble / 优化轮 **是否调用** 仍由 **`planner_round_fsm`** + **`post_parse_pipeline_fsm`** 计算；
//! 本模块仅收拢解析分支，避免 `mod.rs` 深层嵌套 `match`。
//!
//! 见 `docs/design/per_state_machine_consolidation.md`（分阶段回合编排）。

use crate::agent::plan_artifact::{AgentReplyPlanV1, PlanArtifactError};
use crate::types::Message;

use super::staged_parse_terminal::StagedParseTerminalRoute;

/// 首轮规划解析完成后，对 **`run_staged_plan_with_prepared_request`** 的三向路由（**不含** assistant `Message`；
/// 调用方保留单次 LLM 返回的 `msg` 供 `push` / `outer_loop`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedPlannerRoute {
    /// `resolve_parse_with_assistant` → **`QuietFinish`**（重入且无结构化计划时收敛）。
    QuietFinish,
    /// 解析失败或非结构化回复 → 降级 **`run_agent_outer_loop`**。
    DegradeToOuterLoop,
    /// 首轮 `NotFound` 且已有实质只读概览终答 → 落盘 assistant 后结束本分阶段回合（**不**外循环）。
    FinishWithDirectPlannerAnswer,
    /// 已得合法 **`AgentReplyPlanV1`** → **`prepared_post_parse_schedule`** 及后续管线。
    ContinueWithPlan { plan: AgentReplyPlanV1 },
}

impl PreparedPlannerRoute {
    pub(crate) fn as_static_str(&self) -> &'static str {
        match self {
            Self::QuietFinish => "quiet_finish",
            Self::DegradeToOuterLoop => "degrade_to_outer_loop",
            Self::FinishWithDirectPlannerAnswer => "finish_with_direct_planner_answer",
            Self::ContinueWithPlan { .. } => "continue_with_plan",
        }
    }
}

/// 合并 **`resolve_parse_with_assistant`** 与降级路径日志，产出 **`PreparedPlannerRoute`**（无 IO）。
/// `assistant_msg` 仅用于传入解析器（克隆一次）；调用方保留原始 **`Message`** 供后续 `push`。
pub(crate) fn resolve_prepared_planner_route(
    parse_result: Result<AgentReplyPlanV1, PlanArtifactError>,
    entered_from_step_execution_round: bool,
    assistant_msg: &Message,
    merged_for_log: String,
    parse_err_detail: Option<String>,
    degrade_like_not_found: bool,
    user_task: Option<&str>,
) -> PreparedPlannerRoute {
    match resolve_parse_with_assistant(
        parse_result,
        entered_from_step_execution_round,
        merged_for_log.as_str(),
        user_task,
        assistant_msg.clone(),
    ) {
        PreparedPlannerParseOutcome::ContinueWithPlan { plan } => {
            PreparedPlannerRoute::ContinueWithPlan { plan }
        }
        PreparedPlannerParseOutcome::QuietFinish => PreparedPlannerRoute::QuietFinish,
        PreparedPlannerParseOutcome::FinishWithDirectPlannerAnswer => {
            log::info!(
                target: "crabmate",
                "分阶段规划：首轮无 agent_reply_plan JSON，但已产出只读概览类实质终答（merged_len={}）；跳过外循环降级",
                merged_for_log.chars().count(),
            );
            PreparedPlannerRoute::FinishWithDirectPlannerAnswer
        }
        PreparedPlannerParseOutcome::DegradeToOuterLoop => {
            if degrade_like_not_found {
                log::debug!(
                    target: "crabmate",
                    "分阶段规划未产出结构化任务 (可能是通识问答或直接回复) merged_len={} merged_preview={}；降级为常规循环",
                    merged_for_log.chars().count(),
                    crate::redact::preview_chars(
                        merged_for_log.as_str(),
                        crate::redact::MESSAGE_LOG_PREVIEW_CHARS,
                    )
                );
            } else {
                log::warn!(
                    target: "crabmate",
                    "staged_plan_invalid parse_err={} merged_len={} merged_preview={}；降级为常规工具循环",
                    parse_err_detail.unwrap_or_default(),
                    merged_for_log.chars().count(),
                    crate::redact::preview_chars(
                        merged_for_log.as_str(),
                        crate::redact::MESSAGE_LOG_PREVIEW_CHARS,
                    )
                );
            }
            PreparedPlannerRoute::DegradeToOuterLoop
        }
    }
}

/// 解析一步的终端路由（调用方执行 IO：`push` / `outer_loop` / 继续后续 pipeline）。
#[derive(Debug)]
pub(crate) enum PreparedPlannerParseOutcome {
    /// 已得 **`AgentReplyPlanV1`**，进入 **no_task / ensemble / 优化 / 步循环**。
    ContinueWithPlan { plan: AgentReplyPlanV1 },
    /// **`QuietFinishOnPlanNotFound`**：本分阶段回合静默结束。
    QuietFinish,
    /// 降级到常规 **`run_agent_outer_loop`**；调用方使用外层已持有的 **`msg`** 写入历史。
    DegradeToOuterLoop,
    /// 首轮只读概览已直接作答：调用方将 assistant 写入历史后结束。
    FinishWithDirectPlannerAnswer,
}

/// 表驱动：对等旧实现中 **`parse_result` + `staged_planner_parse_route`** 分支。
pub(crate) fn resolve_parse_with_assistant(
    parse_result: Result<AgentReplyPlanV1, PlanArtifactError>,
    entered_from_step_execution_round: bool,
    merged_answer_text: &str,
    user_task: Option<&str>,
    _assistant_msg_for_api_compat: Message,
) -> PreparedPlannerParseOutcome {
    match parse_result {
        Ok(plan) => PreparedPlannerParseOutcome::ContinueWithPlan { plan },
        Err(parse_err) => {
            let parse_route = super::planner_parse_fsm::staged_planner_parse_route(
                &parse_err,
                entered_from_step_execution_round,
                merged_answer_text,
                user_task,
            );
            match StagedParseTerminalRoute::from_planner_parse_route(parse_route) {
                StagedParseTerminalRoute::QuietFinish => PreparedPlannerParseOutcome::QuietFinish,
                StagedParseTerminalRoute::FinishWithDirectAnswer => {
                    PreparedPlannerParseOutcome::FinishWithDirectPlannerAnswer
                }
                StagedParseTerminalRoute::DegradeToOuterLoop => {
                    PreparedPlannerParseOutcome::DegradeToOuterLoop
                }
                StagedParseTerminalRoute::ContinueWithPlan { .. } => {
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
        let o = resolve_parse_with_assistant(
            Ok(plan.clone()),
            false,
            "",
            None,
            Message::user_only("u"),
        );
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
            "",
            None,
            Message::user_only("u"),
        );
        assert!(matches!(o, PreparedPlannerParseOutcome::QuietFinish));
    }

    #[test]
    fn resolve_prepared_route_ok_matches_continue() {
        let plan = minimal_plan();
        let msg = Message::user_only("u");
        let r = resolve_prepared_planner_route(
            Ok(plan.clone()),
            false,
            &msg,
            String::new(),
            None,
            false,
            None,
        );
        assert!(matches!(r, PreparedPlannerRoute::ContinueWithPlan { .. }));
        assert_eq!(r.as_static_str(), "continue_with_plan");
    }

    #[test]
    fn resolve_prepared_route_not_found_entered_is_quiet() {
        let msg = Message::user_only("u");
        let r = resolve_prepared_planner_route(
            Err(PlanArtifactError::NotFound),
            true,
            &msg,
            String::new(),
            None,
            true,
            None,
        );
        assert!(matches!(r, PreparedPlannerRoute::QuietFinish));
        assert_eq!(r.as_static_str(), "quiet_finish");
    }

    #[test]
    fn resolve_prepared_route_not_found_not_entered_is_degrade() {
        let msg = Message::user_only("x");
        let r = resolve_prepared_planner_route(
            Err(PlanArtifactError::NotFound),
            false,
            &msg,
            "short".to_string(),
            Some("nf".into()),
            true,
            Some("分析当前项目"),
        );
        assert!(matches!(r, PreparedPlannerRoute::DegradeToOuterLoop));
        assert_eq!(r.as_static_str(), "degrade_to_outer_loop");
    }

    #[test]
    fn resolve_prepared_route_not_found_substantive_readonly_finishes() {
        let msg = Message::user_only("u");
        let merged = "好的，我来分析当前项目。\n\n## 项目总览\n\n".repeat(20);
        let r = resolve_prepared_planner_route(
            Err(PlanArtifactError::NotFound),
            false,
            &msg,
            merged,
            Some("nf".into()),
            true,
            Some("分析当前项目"),
        );
        assert!(matches!(
            r,
            PreparedPlannerRoute::FinishWithDirectPlannerAnswer
        ));
        assert_eq!(r.as_static_str(), "finish_with_direct_planner_answer");
    }
}
