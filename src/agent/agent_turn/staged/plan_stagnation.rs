//! 分阶段滚动视界：步执行完成后规划轮再次产出**与上轮相同**的 `agent_reply_plan` 时的拦截与反馈。

use crate::agent::plan_artifact::{
    AgentReplyPlanV1, PlanStepV1, parse_agent_reply_plan_v1_from_assistant_message,
    plan_steps_fingerprint,
};
use crate::types::{Message, message_content_as_str};

use super::empty_execution::staged_step_window_has_tool;

use crabmate_display_rules::STAGED_REPEATED_PLAN_ORCHESTRATION_PREFIX;

/// 注入给规划轮的 user 正文前缀（用于统计已自动纠偏次数）。
pub(super) const STAGED_REPEATED_PLAN_COACH_PREFIX: &str =
    STAGED_REPEATED_PLAN_ORCHESTRATION_PREFIX;

/// 同一指纹规划在步后重入时，允许自动注入反馈并**重新规划**的次数上界（不含首次执行）。
const MAX_STAGED_IDENTICAL_PLAN_AUTO_REPLANS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StagedPlanStagnationAction {
    /// 注入反馈 user 后应重新发起无工具规划轮（**勿**执行本重复计划）。
    ReplanWithFeedback(String),
    /// 已多次纠偏仍重复，结束本分阶段回合以免刷屏。
    StopAfterRepeatedPlan,
}

fn last_parsed_agent_reply_plan_in_messages(messages: &[Message]) -> Option<AgentReplyPlanV1> {
    for m in messages.iter().rev() {
        if m.role != "assistant" {
            continue;
        }
        if let Ok(plan) = parse_agent_reply_plan_v1_from_assistant_message(m) {
            return Some(plan);
        }
    }
    None
}

fn stagnation_coach_injections_count(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            m.role == "user"
                && message_content_as_str(&m.content)
                    .is_some_and(|c| c.starts_with(STAGED_REPEATED_PLAN_COACH_PREFIX))
        })
        .count()
}

fn step_id_had_tool_execution_in_messages(messages: &[Message], step_id: &str) -> bool {
    let id_trim = step_id.trim();
    if id_trim.is_empty() {
        return false;
    }
    let id_line = format!("- id: {id_trim}");
    for (i, m) in messages.iter().enumerate() {
        if m.role != "user" {
            continue;
        }
        let Some(content) = message_content_as_str(&m.content) else {
            continue;
        };
        if content.contains("### 分步") && content.contains(id_line.as_str()) {
            return staged_step_window_has_tool(messages, i);
        }
    }
    false
}

fn all_plan_steps_already_executed_with_tools(messages: &[Message], steps: &[PlanStepV1]) -> bool {
    !steps.is_empty()
        && steps
            .iter()
            .all(|s| step_id_had_tool_execution_in_messages(messages, s.id.as_str()))
}

fn repeated_plan_feedback_user_body(plan: &AgentReplyPlanV1) -> String {
    let step_lines: Vec<String> = plan
        .steps
        .iter()
        .map(|s| format!("- `{}`：{}", s.id.trim(), s.description.trim()))
        .collect();
    format!(
        "{STAGED_REPEATED_PLAN_COACH_PREFIX}\n\
         上一轮分步已执行且对话中已有对应工具结果，但本轮规划与上一轮**完全相同**（步 id / executor_kind 一致）。\n\
         **禁止**再次规划并执行下列已完成的步：\n\
         {}\n\n\
         请根据对话中的工具输出与用户原始目标，规划**下一步**（例如 `patch_write` 改代码、`test_runner` 编译/测试），\
         若只读信息已足够则返回 `no_task: true`、`steps: []` 并用简短正文总结；勿重复 `review_readonly` 读同一文档。",
        step_lines.join("\n")
    )
}

/// 步执行轮结束后再次进入规划轮时，检测「相同计划 + 步内已有工具」的停滞。
pub(super) fn evaluate_staged_plan_stagnation_after_step_round(
    messages: &[Message],
    new_plan: &AgentReplyPlanV1,
    entered_from_step_execution_round: bool,
) -> Option<StagedPlanStagnationAction> {
    if !entered_from_step_execution_round || new_plan.no_task || new_plan.steps.is_empty() {
        return None;
    }
    let prev = last_parsed_agent_reply_plan_in_messages(messages)?;
    if plan_steps_fingerprint(&prev.steps) != plan_steps_fingerprint(&new_plan.steps) {
        return None;
    }
    if !all_plan_steps_already_executed_with_tools(messages, new_plan.steps.as_slice()) {
        return None;
    }
    let coach_count = stagnation_coach_injections_count(messages);
    if coach_count >= MAX_STAGED_IDENTICAL_PLAN_AUTO_REPLANS {
        return Some(StagedPlanStagnationAction::StopAfterRepeatedPlan);
    }
    Some(StagedPlanStagnationAction::ReplanWithFeedback(
        repeated_plan_feedback_user_body(new_plan),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::{PlanStepExecutorKind, PlanStepV1};
    use crate::types::MessageContent;

    fn plan_one_readonly() -> AgentReplyPlanV1 {
        AgentReplyPlanV1 {
            plan_type: "agent_reply_plan".to_string(),
            version: 1,
            steps: vec![PlanStepV1 {
                id: "read-hpcg-readme".to_string(),
                description: "读取 README".to_string(),
                workflow_node_id: None,
                executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            }],
            no_task: false,
        }
    }

    fn assistant_with_plan(plan: &AgentReplyPlanV1) -> Message {
        let json = serde_json::to_string(plan).expect("json");
        Message::assistant_only(format!("计划\n```json\n{json}\n```"))
    }

    fn step_user(step_id: &str) -> Message {
        Message::user_only(format!("### 分步 1/1\n- id: {step_id}\n- 描述: 读 README"))
    }

    fn tool_read() -> Message {
        Message {
            role: "tool".into(),
            content: Some(MessageContent::Text("ok".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some("read_file".into()),
            tool_call_id: None,
        }
    }

    #[test]
    fn detects_repeat_after_step_tools_and_requests_replan() {
        let plan = plan_one_readonly();
        let messages = vec![
            assistant_with_plan(&plan),
            step_user("read-hpcg-readme"),
            tool_read(),
        ];
        let action = evaluate_staged_plan_stagnation_after_step_round(&messages, &plan, true);
        assert_eq!(
            action,
            Some(StagedPlanStagnationAction::ReplanWithFeedback(
                repeated_plan_feedback_user_body(&plan)
            ))
        );
    }

    #[test]
    fn no_action_on_first_round() {
        let plan = plan_one_readonly();
        let action = evaluate_staged_plan_stagnation_after_step_round(&[], &plan, false);
        assert_eq!(action, None);
    }

    #[test]
    fn stops_after_max_coach_injections() {
        let plan = plan_one_readonly();
        let messages = vec![
            assistant_with_plan(&plan),
            step_user("read-hpcg-readme"),
            tool_read(),
            Message::user_only(format!("{STAGED_REPEATED_PLAN_COACH_PREFIX}\n已提示")),
            Message::user_only(format!("{STAGED_REPEATED_PLAN_COACH_PREFIX}\n已提示2")),
        ];
        let action = evaluate_staged_plan_stagnation_after_step_round(&messages, &plan, true);
        assert_eq!(
            action,
            Some(StagedPlanStagnationAction::StopAfterRepeatedPlan)
        );
    }
}
