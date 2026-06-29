use crate::agent::agent_turn::completion_suppression::plan_steps_are_redundant_after_completion;
use crate::agent::agent_turn::params::RunLoopParams;
use crate::agent::agent_turn::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
};
use crate::agent::plan_artifact::PlanStepV1;
use tracing::info;

pub(super) fn should_suppress_completed_replanning(
    p: &mut RunLoopParams<'_>,
    entered_from_step_execution_round: bool,
    steps: &[PlanStepV1],
) -> bool {
    if !entered_from_step_execution_round || steps.is_empty() {
        return false;
    }
    let satisfied = matches!(
        check_active_user_goal_completion_evidence(p.turn.messages()),
        GoalCompletionEvidenceCheck::Satisfied
    );
    if !satisfied || !plan_steps_are_redundant_after_completion(steps) {
        return false;
    }
    let goal_preview =
        crate::agent::plan_optimizer::staged_plan_trigger_user_content(p.turn.messages())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
    info!(
        target: "crabmate::staged",
        goal_preview = %goal_preview,
        step_count = steps.len(),
        "当前用户目标已有完成证据，抑制步后重复规划"
    );
    true
}

#[cfg(test)]
mod tests {
    use crate::agent::agent_turn::completion_suppression::plan_steps_are_redundant_after_completion;
    use crate::agent::agent_turn::task_level_evidence::{
        GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    };
    use crate::agent::plan_artifact::PlanStepV1;
    use crate::types::Message;

    fn step(id: &str, kind: Option<&str>, description: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: description.to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: kind.map(str::to_string),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    #[test]
    fn suppresses_verification_and_summary_plan_after_completion() {
        let steps = vec![
            step("rerun-demo", Some("verify"), "重新运行 demo 确认输出"),
            step("verify-product-exists", None, "检查产物是否存在"),
            step("final-summary", Some("summary"), "汇报最终结果"),
        ];

        assert!(plan_steps_are_redundant_after_completion(&steps));
    }

    #[test]
    fn does_not_suppress_followup_write_or_fix_plan() {
        let steps = vec![step(
            "fix-tests",
            Some("implement"),
            "修复失败测试并修改实现",
        )];

        assert!(!plan_steps_are_redundant_after_completion(&steps));
    }

    #[test]
    fn multi_turn_compile_not_suppressed_by_earlier_readonly_evidence() {
        fn msg(role: &str, text: &str) -> Message {
            Message {
                role: role.to_string(),
                content: Some(text.into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: None,
            }
        }

        let messages = vec![
            msg("user", "分析当前目录"),
            msg("assistant", "当前目录包含三个压缩包，分析完成。"),
            msg("user", "编译hpcg"),
            msg("assistant", "好的，开始编译。"),
        ];
        let steps = vec![step("verify-build", Some("verify"), "检查编译产物是否存在")];

        assert!(plan_steps_are_redundant_after_completion(&steps));
        assert!(!matches!(
            check_active_user_goal_completion_evidence(&messages),
            GoalCompletionEvidenceCheck::Satisfied
        ));
    }
}
