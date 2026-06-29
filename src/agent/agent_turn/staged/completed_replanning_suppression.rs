use crate::agent::agent_turn::params::RunLoopParams;
use crate::agent::agent_turn::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
};
use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepV1};

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
    satisfied && plan_steps_are_redundant_after_completion(steps)
}

fn plan_steps_are_redundant_after_completion(steps: &[PlanStepV1]) -> bool {
    steps.iter().all(plan_step_is_redundant_after_completion)
}

fn plan_step_is_redundant_after_completion(step: &PlanStepV1) -> bool {
    let text = redundant_plan_step_text(step);
    if contains_followup_write_or_fix_marker(&text) {
        return false;
    }
    step.acceptance
        .as_ref()
        .is_some_and(PlanStepAcceptance::is_effective)
        || contains_redundant_plan_step_marker(&text)
}

fn redundant_plan_step_text(step: &PlanStepV1) -> String {
    format!(
        "{}\n{}\n{}",
        step.id,
        step.step_kind.as_deref().unwrap_or_default(),
        step.description
    )
    .to_lowercase()
}

fn contains_followup_write_or_fix_marker(text: &str) -> bool {
    [
        "implement",
        "implementation",
        "patch",
        "write",
        "modify",
        "edit",
        "change",
        "fix",
        "repair",
        "refactor",
        "create",
        "add",
        "delete",
        "remove",
        "实现",
        "编写",
        "修改",
        "修复",
        "新增",
        "创建",
        "删除",
        "重构",
        "调整",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn contains_redundant_plan_step_marker(text: &str) -> bool {
    [
        "verify",
        "verification",
        "validate",
        "validation",
        "check",
        "confirm",
        "ensure",
        "exist",
        "exists",
        "rerun",
        "re-run",
        "run again",
        "list",
        "inspect",
        "read",
        "review",
        "summarize",
        "summary",
        "final",
        "report",
        "test",
        "验收",
        "验证",
        "校验",
        "确认",
        "检查",
        "确保",
        "存在",
        "重跑",
        "重新运行",
        "再运行",
        "列出",
        "查看",
        "读取",
        "复查",
        "总结",
        "汇报",
        "最终",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::plan_steps_are_redundant_after_completion;
    use crate::agent::plan_artifact::PlanStepV1;

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
        use super::GoalCompletionEvidenceCheck;
        use crate::agent::agent_turn::task_level_evidence::check_active_user_goal_completion_evidence;
        use crate::types::Message;

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
