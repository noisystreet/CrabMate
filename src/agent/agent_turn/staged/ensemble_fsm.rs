//! 逻辑多规划员（ensemble）内部：**辅助规划员串行轮次**解析结果与 **合并轮** 步列表的纯决策。
//! 与 `maybe_run_staged_plan_ensemble_then_merge` 对齐；**不**发起 LLM。

use crate::agent::plan_artifact::{AgentReplyPlanV1, PlanArtifactError, PlanStepV1};

/// 单次辅助规划员 assistant 经 `parse_agent_reply_plan_v1_*` 后的结果（采纳草案或终止链）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnsembleSecondaryPlannerRoundOutcome {
    /// `no_task == false` 且 `steps` 非空：草案有效，追加到 `accepted` 并继续下一轮（若仍有配额）。
    AcceptAppend(AgentReplyPlanV1),
    /// 解析失败、`no_task`、或 `steps` 为空：停止追加后续辅助规划员，保留已收集草案。
    StopChain,
}

/// 与 `mod.rs` 内「解析 → 采纳或 `break`」等价；枚举携带采纳的规划，避免驱动层重复分支。
pub(crate) fn ensemble_secondary_planner_round_outcome(
    parsed: Result<AgentReplyPlanV1, PlanArtifactError>,
) -> EnsembleSecondaryPlannerRoundOutcome {
    match parsed {
        Ok(p) if !p.no_task && !p.steps.is_empty() => {
            EnsembleSecondaryPlannerRoundOutcome::AcceptAppend(p)
        }
        Ok(_) | Err(_) => EnsembleSecondaryPlannerRoundOutcome::StopChain,
    }
}

/// 合并轮（`try_parse_ensemble_planner_reply`）解析出步列表后的应用方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnsembleMergeOutcome {
    /// 用解析出的步列表覆盖当前主规划（调用方负责 `push_assistant_merging_trailing_empty_placeholder` 等副作用）。
    AppliedSteps(Vec<PlanStepV1>),
    /// 无效或无可执行步：保留合并前的 `plan`，由调用方弹出合并轮 coach user。
    KeepPriorPlan,
}

/// 将 `plan_ensemble::try_parse_ensemble_planner_reply` 的返回值转为合并效果（单处权威分支）。
pub(crate) fn ensemble_merge_outcome_from_parsed_steps(
    merged_steps: Option<Vec<PlanStepV1>>,
) -> EnsembleMergeOutcome {
    match merged_steps {
        Some(steps) if !steps.is_empty() => EnsembleMergeOutcome::AppliedSteps(steps),
        Some(_) | None => EnsembleMergeOutcome::KeepPriorPlan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;

    fn sample_step(id: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: "d".to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    fn sample_plan(no_task: bool, steps: Vec<PlanStepV1>) -> AgentReplyPlanV1 {
        AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps,
            no_task,
        }
    }

    #[test]
    fn secondary_accepts_nonempty_non_no_task() {
        let p = sample_plan(false, vec![sample_step("s1")]);
        let parsed = Ok(p.clone());
        match ensemble_secondary_planner_round_outcome(parsed) {
            EnsembleSecondaryPlannerRoundOutcome::AcceptAppend(got) => assert_eq!(got, p),
            EnsembleSecondaryPlannerRoundOutcome::StopChain => panic!("expected AcceptAppend"),
        }
    }

    #[test]
    fn secondary_stops_on_no_task() {
        let p = sample_plan(true, vec![]);
        assert!(matches!(
            ensemble_secondary_planner_round_outcome(Ok(p)),
            EnsembleSecondaryPlannerRoundOutcome::StopChain
        ));
    }

    #[test]
    fn secondary_stops_on_empty_steps() {
        let p = sample_plan(false, vec![]);
        assert!(matches!(
            ensemble_secondary_planner_round_outcome(Ok(p)),
            EnsembleSecondaryPlannerRoundOutcome::StopChain
        ));
    }

    #[test]
    fn secondary_stops_on_parse_err() {
        assert!(matches!(
            ensemble_secondary_planner_round_outcome(Err(PlanArtifactError::NotFound)),
            EnsembleSecondaryPlannerRoundOutcome::StopChain
        ));
    }

    #[test]
    fn merge_applies_nonempty_steps() {
        let steps = vec![sample_step("a")];
        match ensemble_merge_outcome_from_parsed_steps(Some(steps.clone())) {
            EnsembleMergeOutcome::AppliedSteps(s) => assert_eq!(s, steps),
            EnsembleMergeOutcome::KeepPriorPlan => panic!("expected AppliedSteps"),
        }
    }

    #[test]
    fn merge_keeps_prior_on_none_or_empty() {
        assert_eq!(
            ensemble_merge_outcome_from_parsed_steps(None),
            EnsembleMergeOutcome::KeepPriorPlan
        );
        assert_eq!(
            ensemble_merge_outcome_from_parsed_steps(Some(vec![])),
            EnsembleMergeOutcome::KeepPriorPlan
        );
    }
}
